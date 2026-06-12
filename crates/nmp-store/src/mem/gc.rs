//! Claim / release / `gc_step` for `MemEventStore`.
//!
//! Implements the `HotSet` semantics from `docs/design/lmdb/gc.md` §2:
//!   - per-view ceiling: `DEFAULT_VIEW_CEILING` (1000 events).
//!   - global pinned ceiling: `MAX_PINNED_TOTAL` (20000 events).
//!   - `BTreeSet` idempotency per T25: re-claiming a known id is a no-op.
//!   - `StoreError::OverPinned` on breach (D8).
//!
//! V-60: LRU eviction — when the store exceeds `budget.max_total_events`,
//! `gc_step` evicts the least-recently-accessed (by `access_seq`) events that
//! are not currently pinned (claimed), until the store is at or under the
//! ceiling.  No tombstone is created for LRU-evicted events (they are still
//! valid; the caller may re-fetch them from a relay).

use std::collections::BTreeSet;

use super::{
    access_remove, bytes_to_hex, relay_index_remove, MemEventStore, DEFAULT_VIEW_CEILING,
    MAX_PINNED_TOTAL, TOMBSTONE_MAX_AGE_SECS,
};
use crate::types::{ClaimerId, EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

pub(super) fn register_view_cover(
    store: &MemEventStore,
    claimer: ClaimerId,
    cover_budget: usize,
) -> Result<(), StoreError> {
    let mut st = store.lock()?;
    st.claim_budgets.insert(claimer, cover_budget);
    Ok(())
}

pub(super) fn claim(
    store: &MemEventStore,
    claimer: ClaimerId,
    ids: &[EventId],
) -> Result<(), StoreError> {
    let mut st = store.lock()?;
    let ceiling = *st
        .claim_budgets
        .get(&claimer)
        .unwrap_or(&DEFAULT_VIEW_CEILING);

    let existing_set = st.claims.entry(claimer).or_default();
    // Use BTreeSet for intra-call deduplication so repeated ids in the same
    // batch do not count multiple times toward the per-view ceiling (T25).
    let new_ids: BTreeSet<String> = ids
        .iter()
        .map(|id| bytes_to_hex(id))
        .filter(|hex| !existing_set.contains(hex))
        .collect();

    let current_for_claimer = existing_set.len();
    let requested_for_claimer = current_for_claimer + new_ids.len();
    if requested_for_claimer > ceiling {
        return Err(StoreError::OverPinned {
            claimer,
            requested: requested_for_claimer,
            ceiling,
        });
    }

    // Global pinned ceiling uses UNION of all claim sets, not SUM, to avoid
    // double-counting ids pinned by multiple claimers (D8 / gc.md §2).
    let current_global: BTreeSet<&str> =
        st.claims.values().flatten().map(String::as_str).collect();
    let global_new = new_ids
        .iter()
        .filter(|hex| !current_global.contains(hex.as_str()))
        .count();
    let requested_global = current_global.len() + global_new;
    if requested_global > MAX_PINNED_TOTAL {
        return Err(StoreError::OverPinned {
            claimer,
            requested: requested_global,
            ceiling: MAX_PINNED_TOTAL,
        });
    }

    // Apply the claims.
    let set = st.claims.entry(claimer).or_default();
    for hex in new_ids {
        set.insert(hex);
    }
    Ok(())
}

pub(super) fn release(store: &MemEventStore, claimer: ClaimerId) -> Result<(), StoreError> {
    let mut st = store.lock()?;
    st.claims.remove(&claimer);
    st.claim_budgets.remove(&claimer);
    Ok(())
}

/// One bounded GC pass.
///
/// `now_secs` is the kernel clock as Unix seconds (D7 — caller-supplied, never
/// read from `SystemTime::now()` here).
///
/// Three phases, in order:
/// 1. Reap NIP-40 expired events (up to `budget.max_events_per_step`).
/// 2. LRU-evict un-pinned events when store size exceeds `budget.max_total_events`.
/// 3. Purge tombstone rows older than `TOMBSTONE_MAX_AGE_SECS`.
pub(super) fn gc_step(
    store: &MemEventStore,
    budget: GcBudget,
    now_secs: u64,
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
    // Pinned events (union of all `claims` sets) are never evicted — that would
    // violate the `claim`/`release` contract.
    //
    // No tombstone is created for LRU-evicted events: they are still valid Nostr
    // events; tombstoning them would permanently block legitimate re-insertion.
    if st.events.len() > budget.max_total_events {
        // Build the pinned set once.
        let pinned: BTreeSet<&str> =
            st.claims.values().flatten().map(String::as_str).collect();

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
    use crate::types::EventId;
    use crate::{EventStore, MemEventStore};

    fn make_id(b: u8) -> EventId {
        let mut id = [0u8; 32];
        id[0] = b;
        id
    }

    #[test]
    fn claim_idempotent_reclaim_does_not_count() {
        let store = MemEventStore::new();
        let c = ClaimerId(1);
        store.register_view_cover(c, 5).unwrap();
        let id = make_id(1);
        store.claim(c, &[id]).unwrap();
        // Re-claiming the same id must not count toward the ceiling.
        store.claim(c, &[id]).unwrap();
        let st = store.lock().unwrap();
        assert_eq!(
            st.claims[&c].len(),
            1,
            "idempotent: re-claim must not add entry"
        );
    }

    #[test]
    fn claim_over_per_view_ceiling_returns_err() {
        let store = MemEventStore::new();
        let c = ClaimerId(2);
        store.register_view_cover(c, 2).unwrap();
        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
        let result = store.claim(c, &[make_id(3)]);
        assert!(
            matches!(result, Err(StoreError::OverPinned { .. })),
            "must return OverPinned when per-view ceiling exceeded"
        );
    }

    #[test]
    fn release_clears_all_pins() {
        let store = MemEventStore::new();
        let c = ClaimerId(3);
        store.register_view_cover(c, 100).unwrap();
        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
        store.release(c).unwrap();
        let st = store.lock().unwrap();
        assert!(
            !st.claims.contains_key(&c),
            "release must clear claimer's pins"
        );
    }

    #[test]
    fn claim_intra_call_dedup_counts_once() {
        // Passing the same id three times in one batch must increment the
        // per-view ceiling by exactly 1, not 3.
        let store = MemEventStore::new();
        let c = ClaimerId(4);
        store.register_view_cover(c, 2).unwrap();
        let id = make_id(42);
        // Ceiling is 2; passing the same id three times should only consume 1 slot.
        store.claim(c, &[id, id, id]).unwrap();
        let st = store.lock().unwrap();
        assert_eq!(
            st.claims[&c].len(),
            1,
            "intra-call dup ids must count as one"
        );
    }

    #[test]
    fn release_also_clears_budget() {
        // After release(), claim_budgets must not retain the stale entry.
        let store = MemEventStore::new();
        let c = ClaimerId(5);
        store.register_view_cover(c, 10).unwrap();
        store.claim(c, &[make_id(7)]).unwrap();
        store.release(c).unwrap();
        let st = store.lock().unwrap();
        assert!(!st.claims.contains_key(&c), "release must clear pins");
        assert!(
            !st.claim_budgets.contains_key(&c),
            "release must clear budget entry"
        );
    }

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
        let report = gc_step(&store, budget, now_secs).unwrap();

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
        let report = gc_step(&store, budget, now_secs).unwrap();

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
