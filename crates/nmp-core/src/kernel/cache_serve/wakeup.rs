//! Cache-serve wakeup DRAIN (#1520).
//!
//! This is the **drain** half of the #1520 event-driven cache-serve wakeup. It
//! lives inside the `cache_serve` module (not `kernel::store_wakeup`) for one
//! reason: it re-enqueues via `enqueue_interest_cache_serve_deferred`, which is
//! sealed module-private to `cache_serve/mod.rs` (the only production enqueue
//! path is `Kernel::register_interest`; a child module may reach its parent's
//! private items, an outside module may not).
//!
//! The wake **arm** (`note_store_mutation`), the `StoreWakeups` owner, and the
//! pull wake arm all live in `kernel::store_wakeup` (ADR-0072 §10). The set
//! drained here is `store_wakeups.cache_serve` — the same `BTreeSet<u64>` of
//! already-served interest completion keys the #1520 mechanism always used.

use super::super::Kernel;

impl Kernel {
    /// Drain the coalesced cache-serve wakeup set, re-arming each affected
    /// interest for a fresh cache-serve pass.
    ///
    /// Called as the first action of `run_cache_serve_step`. For each wakeup
    /// key:
    /// 1. Remove from `served_interest_shapes` so `enqueue_cache_serve` won't
    ///    skip the interest.
    /// 2. Find the matching `(SubKey, LogicalInterest)` in the active registry.
    /// 3. Re-enqueue via `enqueue_interest_cache_serve_deferred`.
    ///
    /// After draining, the set is empty until the next `note_store_mutation`
    /// call fires new matches. (#1520 — behavior preserved byte-for-byte.)
    pub(in crate::kernel) fn drain_cache_serve_wakeups(&mut self) {
        if self.store_wakeups.cache_serve.is_empty() {
            return;
        }
        // Collect wakeup keys to process (drain the set atomically).
        let wakeup_keys: Vec<u64> = self.store_wakeups.cache_serve.iter().copied().collect();
        self.store_wakeups.cache_serve.clear();

        // Snapshot active interests once to avoid repeated registry reads.
        let active = self.lifecycle.registry().iter_active_with_keys();

        for wakeup_key in wakeup_keys {
            // Remove from the completion set so enqueue_cache_serve won't skip.
            self.served_interest_shapes.remove(&wakeup_key);

            // Find the interest matching this wakeup key in the current registry.
            // If it has been dropped (last owner left) since the wakeup was armed,
            // there is no entry to re-enqueue — silently skip.
            for (sub_key, interest) in &active {
                let candidate_key = super::completion_key_for_interest(sub_key, &interest.shape);
                if candidate_key == wakeup_key {
                    self.enqueue_interest_cache_serve_deferred(sub_key, &interest.shape);
                    break;
                }
            }
        }
    }
}
