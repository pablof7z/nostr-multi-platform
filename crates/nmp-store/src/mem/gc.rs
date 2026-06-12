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

use super::{access_remove, bytes_to_hex, relay_index_remove, MemEventStore, TOMBSTONE_MAX_AGE_SECS};
use crate::types::{EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
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
pub(super) fn gc_step_with_pins(
    store: &MemEventStore,
    budget: GcBudget,
    now_secs: u64,
    pins: &HashSet<EventId>,
) -> Result<GcReport, StoreError> {
    let start = std::time::Instant::now();
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
            access_remove(&mut *st, id_hex);
            st.tombstones.insert(
                id_hex.clone(),
                TombstoneRow {
                    // ev.raw is a stored (verified) event — id_bytes() is guaranteed Some.
                    target_id: ev.raw.id_bytes().expect("stored event has valid hex id"),
                    kind5_event_id: None,
                    deleter_pubkey: None,
                    deleted_at: now_secs,
                    sources: vec![],
                    origin: TombstoneOrigin::NIP40Expiry,
                },
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

        for (_, id_hex) in candidates.into_iter().take(to_evict) {
            if st.events.remove(&id_hex).is_some() {
                st.provenance.remove(&id_hex);
                relay_index_remove(&mut *st, &id_hex);
                access_remove(&mut *st, &id_hex);
                report.lru_evicted += 1;
            }
            if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                return finish(start, report);
            }
        }
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
fn finish(start: std::time::Instant, mut report: GcReport) -> Result<GcReport, StoreError> {
    report.duration_ms = start.elapsed().as_millis() as u32;
    Ok(report)
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
        let report =
            gc_step_with_pins(&store, budget, now_secs, &HashSet::new()).unwrap();

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
        let report =
            gc_step_with_pins(&store, budget, now_secs, &HashSet::new()).unwrap();

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
