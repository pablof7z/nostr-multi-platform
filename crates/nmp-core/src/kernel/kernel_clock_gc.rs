//! Clock injection + wall-clock accessors + bounded GC pass.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    /// Swap the kernel's wall-clock (test/replay seam; production never calls this).
    // `allow(dead_code)`: `test-support` consumers reach this via the feature
    // gate; in non-test, non-feature builds the method is unreachable by design.
    #[allow(dead_code)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.routing_trace.set_clock(Arc::clone(&clock));
        self.clock = clock;
    }

    // `allow(dead_code)`: the non-test variant is intentionally private and
    // has no in-crate caller; it exists so non-test builds still compile
    // cleanly when `set_clock` is called through a generic seam.
    #[allow(dead_code)]
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.routing_trace.set_clock(Arc::clone(&clock));
        self.clock = clock;
    }

    /// Current wall-clock seconds since the Unix epoch via the injected `Clock` (D9).
    pub fn now_secs(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Current wall-clock milliseconds since the Unix epoch via the injected `Clock` (D9).
    pub(crate) fn now_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Run one bounded GC pass; records the result for diagnostics.
    pub fn run_gc_step(&mut self) -> Option<crate::store::GcReport> {
        let now_secs = self.now_secs();
        // #1088 — RAM-tier eviction runs on every GC pass regardless of
        // whether the store pass succeeds.  This is a separate call site from
        // the LMDB-tier gc_step (#1085) so the two paths stay independent and
        // merge-clean.
        let ram_report = self.evict_ram_caches();
        if ram_report.events_evicted + ram_report.profiles_evicted > 0 {
            tracing::debug!(
                events_evicted = ram_report.events_evicted,
                profiles_evicted = ram_report.profiles_evicted,
                "ram cache eviction pass",
            );
        }
        // #1090 Stage 1 / #1480 — derive the ephemeral store-tier pin set only
        // when a finite durable-retention budget needs it. With production's
        // default unbounded durable retention this returns empty pins and avoids
        // the store scan entirely.
        let (pins, gc_budget) = self.derive_store_gc_inputs();
        // K3 Stage D3 leg 2 — the eviction⇄ledger coherence backstop guards
        // (one per active covered `(filter_hash, relay)`). Passed alongside the
        // pins so the store can lower an over-claimed `covered_through` in the
        // SAME transaction as the below-floor delete that made it stale.
        let coverage_guards = if gc_budget.max_total_events < usize::MAX {
            self.derive_coverage_guards()
        } else {
            Vec::new()
        };
        match self.store.gc_step_with_pins_and_coverage(
            gc_budget,
            now_secs,
            &pins,
            &coverage_guards,
        ) {
            Ok(report) => {
                self.last_gc_at_ms = Some(self.now_ms());
                self.last_gc = Some(report.clone());
                #[cfg(any(test, feature = "test-support"))]
                if report.lru_evicted > 0 {
                    PROCESS_STORE_LRU_EVICTED.fetch_add(
                        report.lru_evicted as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                Some(report)
            }
            Err(e) => {
                tracing::warn!(error = %e, "gc_step failed; skipping this pass");
                None
            }
        }
    }

    /// The last `GcReport` from `run_gc_step`, or `None` if no pass has run yet.
    pub fn last_gc(&self) -> Option<&crate::store::GcReport> {
        self.last_gc.as_ref()
    }

    /// Wall-clock time (Unix ms) of the last `run_gc_step`, or `None`.
    pub fn last_gc_at_ms(&self) -> Option<u64> {
        self.last_gc_at_ms
    }

    /// Test-support: set a durable LRU eviction ceiling for the GC budget.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_gc_budget_ceiling(&mut self, max_events: usize) {
        self.gc_budget_ceiling = Some(max_events);
    }
}
