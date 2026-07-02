//! `gc_step` for `MemEventStore`.
//!
//! V-60: LRU eviction — when the store exceeds `budget.max_total_events`,
//! `gc_step` evicts the least-recently-accessed (by `access_seq`) events that
//! are not in the caller-supplied pin set, until the store is at or under the
//! ceiling.  No tombstone is created for LRU-evicted events (they are still
//! valid; the caller may re-fetch them from a relay).
//!
//! #1090 Stage 1: the persisted-claims machinery
//! (`register_view_cover`/`claim`/`release`, the per-claimer `BTreeSet` pin
//! map, the per-view + global ceilings, `StoreError::OverPinned`) is deleted.
//! The pin set is now supplied per call by the kernel (`pins: &HashSet`),
//! derived from `timeline`, `event_claims`, and the active open-interest
//! registry.

use std::collections::HashSet;

// D20 — use the wasm-safe Instant shim rather than std::time::Instant
// directly. On wasm32-unknown-unknown `std::time::Instant::now()` panics;
// `crate::time::Instant` resolves to `web_time::Instant` (performance.now())
// on wasm32 and to `std::time::Instant` on native (zero-cost re-export).
use crate::time::Instant;

use super::fts::fts_index_remove;
use super::ingest_log;
use super::{
    access_remove, bytes_to_hex, relay_index_remove, relay_kind_remove_id, MemEventStore,
    TOMBSTONE_MAX_AGE_SECS,
};
use crate::ingest_log::DeleteReason;
use crate::types::{CoverageGuard, EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

/// One bounded GC pass with an explicit derived pin set.
///
/// `pins` is the set of event ids to protect from Phase-2 LRU eviction (#1090
/// Stage 1 — derived by the kernel from `timeline`, `event_claims`, and the
/// active open-interest registry).
///
/// `now_secs` is the kernel clock as Unix seconds (D7 — caller-supplied, never
/// read from `SystemTime::now()` here).
///
/// Three phases, in order:
/// 1. Reap NIP-40 expired events (up to `budget.max_events_per_step`).
/// 2. LRU-evict un-pinned events when store size exceeds `budget.max_total_events`.
/// 3. Purge tombstone rows older than `TOMBSTONE_MAX_AGE_SECS`.
///
/// `guards` (K3 Stage D3, ADR-0072 §3.D3) is the eviction⇄ledger coherence
/// backstop: if a Phase-2 LRU eviction deletes an event a guard matches whose
/// `created_at <= covered_through`, the matching coverage row is lowered to just
/// below the oldest evicted covered event, **inside the same `st` lock** as the
/// delete — so the ledger never over-claims a range it no longer holds. An empty
/// slice (the flag-off path) makes the eviction byte-identical to before D3.
pub(super) fn gc_step_with_pins(
    store: &MemEventStore,
    budget: GcBudget,
    now_secs: u64,
    pins: &HashSet<EventId>,
    guards: &[CoverageGuard],
) -> Result<GcReport, StoreError> {
    let start = Instant::now();
    let mut st = store.lock()?;
    let mut report = GcReport::default();

    // ── Phase 1: Reap NIP-40 expired events ──────────────────────────────────
    let expired_ids: Vec<String> = st
        .events
        .iter()
        .filter(|(_, ev)| ev.raw.expiration().is_some_and(|exp| exp <= now_secs))
        .map(|(id, _)| id.clone())
        .take(budget.max_events_per_step)
        .collect();

    for id_hex in &expired_ids {
        if let Some(ev) = st.events.remove(id_hex) {
            st.provenance.remove(id_hex);
            relay_index_remove(&mut *st, id_hex);
            relay_kind_remove_id(&mut *st, id_hex);
            fts_index_remove(&mut *st, id_hex);
            access_remove(&mut *st, id_hex);
            // ev.raw is a stored (verified) event — id_bytes() is guaranteed Some.
            let target_id = ev.raw.id_bytes().expect("stored event has valid hex id");
            st.tombstones.insert(
                id_hex.clone(),
                TombstoneRow {
                    target_id,
                    kind5_event_id: None,
                    deleter_pubkey: None,
                    deleted_at: now_secs,
                    sources: vec![],
                    origin: TombstoneOrigin::NIP40Expiry,
                },
            );
            // BLOCKING 2: emit Deleted(Nip40Expiry) — parity with lmdb/gc.rs:209-218.
            // carrier == target (no kind:5 here; the event expires itself).
            ingest_log::emit_deleted(
                &mut *st,
                target_id,
                target_id,
                DeleteReason::Nip40Expiry,
                now_secs * 1000,
            );
            report.expired_reaped += 1;
        }
        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
            return finish(start, report);
        }
    }

    // ── Phase 2: LRU eviction ─────────────────────────────────────────────────
    //
    // If the store is over the event-count ceiling, evict the un-pinned events
    // with the LOWEST access sequence numbers (oldest reads) until we are at or
    // under the ceiling or we exhaust the per-step budget.
    //
    // Pinned events (the caller-supplied `pins` set) are never evicted — the
    // kernel derives them from the live snapshot working set (#1090 Stage 1).
    //
    // No tombstone is created for LRU-evicted events: they are still valid Nostr
    // events; tombstoning them would permanently block legitimate re-insertion.
    if st.events.len() > budget.max_total_events {
        // Convert the caller's pin set (byte ids) to the hex keying the maps,
        // once, before scanning candidates.
        let pinned: HashSet<String> = pins.iter().map(|id| bytes_to_hex(id)).collect();

        // Collect eviction candidates sorted ascending by access_seq (oldest first).
        // Only include events that exist in both maps and are not pinned.
        let mut candidates: Vec<(u64, String)> = st
            .access_index
            .iter()
            .filter(|(hex, _)| !pinned.contains(hex.as_str()))
            .map(|(hex, &seq)| (seq, hex.clone()))
            .collect();
        candidates.sort_unstable_by_key(|(seq, _)| *seq);

        let overage = st.events.len().saturating_sub(budget.max_total_events);
        let to_evict = overage.min(budget.max_events_per_step);

        // K3 Stage D3 backstop: per guard, the lowest `created_at` of an evicted
        // event that the guard matches AND that sits at/below its
        // `covered_through`. After eviction each such row is lowered to just
        // below that timestamp so the ledger stops claiming the now-evicted
        // range. `None` ⇒ no below-floor covered event was evicted for that
        // guard (row untouched). Computed inside the same `st` lock as the
        // deletes, so the lowering and the deletes are atomic together.
        let mut min_evicted_covered: Vec<Option<u64>> = vec![None; guards.len()];

        for (_, id_hex) in candidates.into_iter().take(to_evict) {
            // Capture the guard-relevant fields BEFORE removing the row.
            let evicted_fields = if guards.is_empty() {
                None
            } else {
                st.events.get(&id_hex).map(|ev| {
                    (
                        ev.raw.pubkey.clone(),
                        ev.raw.kind,
                        ev.raw.created_at,
                        ev.raw.tags.clone(),
                    )
                })
            };
            if st.events.remove(&id_hex).is_some() {
                st.provenance.remove(&id_hex);
                relay_index_remove(&mut *st, &id_hex);
                relay_kind_remove_id(&mut *st, &id_hex);
                fts_index_remove(&mut *st, &id_hex);
                access_remove(&mut *st, &id_hex);
                report.lru_evicted += 1;
                if let Some((author, kind, created_at, tags)) = evicted_fields {
                    for (gi, guard) in guards.iter().enumerate() {
                        if created_at <= guard.covered_through
                            && (guard.matches)(&id_hex, &author, kind, created_at, &tags)
                        {
                            let slot = &mut min_evicted_covered[gi];
                            *slot = Some(slot.map_or(created_at, |m| m.min(created_at)));
                        }
                    }
                }
            }
            if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                lower_coverage_rows(&mut st, guards, &min_evicted_covered);
                return finish(start, report);
            }
        }

        // Atomic with the deletes (same `st` lock): lower every row whose
        // covered range lost a below-floor event this pass.
        lower_coverage_rows(&mut st, guards, &min_evicted_covered);
    }

    // ── Phase 3: Purge old tombstones ─────────────────────────────────────────
    let stale_tombstones: Vec<String> = st
        .tombstones
        .iter()
        .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
        .map(|(k, _)| k.clone())
        .collect();
    report.tombstones_purged = stale_tombstones.len();
    for k in stale_tombstones {
        st.tombstones.remove(&k);
    }

    // ── Phase 3b: Purge old address tombstones ────────────────────────────────
    //
    // addr_tombstones guard param-replaceable re-inserts when an event arrives
    // after the kind:5 `a`-tag delete that covered its coordinate.  The gate is
    // `tomb.deleted_at >= event.created_at` — so any new version with a HIGHER
    // created_at bypasses the gate regardless of whether the tombstone is present.
    // A purged addr tombstone therefore only allows stale copies (created_at <=
    // the original delete timestamp) to re-enter, which is identical to the
    // class of stale re-deliveries the per-id tombstone policy already accepts
    // after 90 days.  Safety: same retention argument as id-tombstones.
    let stale_addr: Vec<String> = st
        .addr_tombstones
        .iter()
        .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
        .map(|(k, _)| k.clone())
        .collect();
    report.addr_tombstones_purged = stale_addr.len();
    for k in stale_addr {
        st.addr_tombstones.remove(&k);
    }

    finish(start, report)
}

#[inline]
fn finish(start: Instant, mut report: GcReport) -> Result<GcReport, StoreError> {
    report.duration_ms = start.elapsed().as_millis() as u32;
    Ok(report)
}

/// K3 Stage D3 backstop: lower each guard's coverage row to just below the
/// oldest evicted covered event for it. `min_evicted_covered[gi]` is the lowest
/// `created_at` of an evicted event the guard matched at/below its
/// `covered_through`; the new bound is `that - 1` (the highest timestamp the
/// ledger can still honestly claim, since the event AT that timestamp is now
/// gone). `created_at == 0` lowers to `0` which `record_coverage` semantics
/// treats as "no coverage", so the row is removed rather than left at a
/// misleading `0`. Called inside the held `st` lock (same critical section as
/// the deletes), so the lowering is atomic with the eviction.
fn lower_coverage_rows(
    st: &mut super::MemState,
    guards: &[CoverageGuard],
    min_evicted_covered: &[Option<u64>],
) {
    for (gi, guard) in guards.iter().enumerate() {
        let Some(oldest_evicted) = min_evicted_covered[gi] else {
            continue;
        };
        let key = (guard.filter_hash.clone(), guard.relay.clone());
        let new_bound = oldest_evicted.saturating_sub(1);
        match st.coverage.get(&key).copied() {
            // Only lower (never raise) and never touch a row already at/below
            // the honest bound — the lowering is downward-only.
            Some(existing) if existing > new_bound => {
                if new_bound == 0 {
                    st.coverage.remove(&key);
                } else {
                    st.coverage.insert(key, new_bound);
                }
            }
            _ => {}
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemEventStore;

    // ── addr_tombstone GC tests (S-2 fix) ────────────────────────────────────

    /// Helper: inject an addr tombstone row directly into MemState.
    fn inject_addr_tombstone(store: &MemEventStore, key: &str, deleted_at: u64) {
        let mut st = store.lock().unwrap();
        st.addr_tombstones.insert(
            key.to_string(),
            TombstoneRow {
                target_id: [0u8; 32],
                kind5_event_id: Some([1u8; 32]),
                deleter_pubkey: Some([2u8; 32]),
                deleted_at,
                sources: vec!["wss://test/".into()],
                origin: crate::types::TombstoneOrigin::Kind5,
            },
        );
    }

    /// Stale addr tombstones (older than TOMBSTONE_MAX_AGE_SECS) survive
    /// gc_step BEFORE the fix — this test was RED on master and is the
    /// failing proof required by the TDD brief.
    ///
    /// After the fix it must be GREEN: the addr tombstone is purged.
    #[test]
    fn mem_stale_addr_tombstone_is_purged_by_gc() {
        let store = MemEventStore::new();
        let key = "30023:aa".to_string() + ":some-dtag";

        // deleted_at is TOMBSTONE_MAX_AGE_SECS + 1 seconds in the past.
        let now_secs = 10_000_000u64;
        let deleted_at = now_secs - TOMBSTONE_MAX_AGE_SECS - 1;

        inject_addr_tombstone(&store, &key, deleted_at);

        // Confirm row is present before GC.
        {
            let st = store.lock().unwrap();
            assert!(
                st.addr_tombstones.contains_key(&key),
                "addr_tombstone must exist before gc_step"
            );
        }

        let budget = crate::types::GcBudget {
            max_events_per_step: 1000,
            max_duration_ms: 10_000,
            max_total_events: usize::MAX,
        };
        let report = gc_step_with_pins(&store, budget, now_secs, &HashSet::new(), &[]).unwrap();

        let st = store.lock().unwrap();
        assert!(
            !st.addr_tombstones.contains_key(&key),
            "stale addr_tombstone must be purged by gc_step"
        );
        assert_eq!(
            report.addr_tombstones_purged, 1,
            "report must count the purged addr_tombstone"
        );
    }

    /// Fresh addr tombstones (younger than TOMBSTONE_MAX_AGE_SECS) must NOT
    /// be purged — they are still needed to gate re-inserts.
    #[test]
    fn mem_fresh_addr_tombstone_is_retained_by_gc() {
        let store = MemEventStore::new();
        let key = "30023:bb".to_string() + ":my-dtag";

        let now_secs = 10_000_000u64;
        // deleted_at is only 1 second in the past (very fresh).
        let deleted_at = now_secs - 1;

        inject_addr_tombstone(&store, &key, deleted_at);

        let budget = crate::types::GcBudget {
            max_events_per_step: 1000,
            max_duration_ms: 10_000,
            max_total_events: usize::MAX,
        };
        let report = gc_step_with_pins(&store, budget, now_secs, &HashSet::new(), &[]).unwrap();

        let st = store.lock().unwrap();
        assert!(
            st.addr_tombstones.contains_key(&key),
            "fresh addr_tombstone must NOT be purged"
        );
        assert_eq!(
            report.addr_tombstones_purged, 0,
            "report must not count fresh addr_tombstone as purged"
        );
    }
}
