//! Test-support GC / store-inspection helpers for the kernel.
//!
//! All items here are gated on `cfg(any(test, feature = "test-support"))` and
//! exist so the manually-run `nmp-stress-harness` can drive the REAL
//! [`Kernel::run_gc_step`] pin-derivation + LRU-eviction machinery and read the
//! resulting store state, WITHOUT these ever appearing on the production C-ABI
//! surface (D0). They reuse the exact production primitives —
//! [`Kernel::derive_store_pin_set`], [`Kernel::derive_coverage_guards`],
//! [`Kernel::evict_ram_caches`], and [`EventStore::gc_step_with_pins_and_coverage`]
//! — so a harness assertion exercises landed behaviour, not a parallel copy.
//!
//! The ONLY deviation from production [`Kernel::run_gc_step`] is the
//! configurable LRU `ceiling`: production hardcodes
//! [`crate::store::HOT_EVENT_CEILING`] (10 000), which would force the harness to
//! ingest >10 000 events to observe a single eviction. The ceiling is itself a
//! [`crate::store::GcBudget`] field, so substituting a small value tests the SAME
//! pin/eviction logic with a handful of events — the pin set, the coverage
//! guards, and the truncation→no-eviction safety are all derived identically.

use super::Kernel;
use crate::store::EventStore;
use std::ops::ControlFlow;

impl Kernel {
    /// Run one bounded GC pass to a custom LRU `ceiling` (test-support).
    ///
    /// Mirrors [`Kernel::run_gc_step`] exactly — RAM-tier eviction, the
    /// floor-coherent store pin set, the eviction⇄ledger coverage guards, and
    /// the truncation→`max_total_events = usize::MAX` safety — but with the LRU
    /// ceiling overridden so a small harness can observe eviction.
    pub(crate) fn run_gc_step_to_ceiling_for_test(
        &mut self,
        ceiling: usize,
    ) -> Option<crate::store::GcReport> {
        let now_secs = self.now_secs();
        let _ = self.evict_ram_caches();
        let (pins, complete) = self.derive_store_pin_set();
        let budget = if complete {
            crate::store::GcBudget {
                max_total_events: ceiling,
                ..crate::store::GcBudget::production()
            }
        } else {
            // Pin scan truncated (#1348) — conservatively skip LRU eviction.
            crate::store::GcBudget {
                max_total_events: usize::MAX,
                ..crate::store::GcBudget::production()
            }
        };
        let coverage_guards = self.derive_coverage_guards();
        match self
            .store
            .gc_step_with_pins_and_coverage(budget, now_secs, &pins, &coverage_guards)
        {
            Ok(report) => {
                self.last_gc = Some(report.clone());
                Some(report)
            }
            Err(e) => {
                self.log(format!("test gc_step failed: {e}"));
                None
            }
        }
    }

    /// The store-tier LRU pin set, rendered as lowercase hex event ids
    /// (test-support). Wraps [`Kernel::derive_store_pin_set`] — the exact set
    /// `run_gc_step` passes to the store — so the harness can assert a specific
    /// event is (or is not) protected from eviction.
    pub(crate) fn store_pin_set_hex_for_test(&self) -> Vec<String> {
        let (pins, _complete) = self.derive_store_pin_set();
        pins.iter().map(|id| hex32(id)).collect()
    }

    /// Count of events currently in the durable store (test-support
    /// "store-size read"). Uses the production [`EventStore::query_visit`] seam
    /// over an unconstrained `KindTime` query (empty `kinds` = any kind), so it
    /// reflects what GC actually evicts from / retains in the durable tier.
    pub(crate) fn store_event_count_for_test(&self) -> usize {
        let query = crate::store::StoreQuery::KindTime {
            kinds: Vec::new(),
            since: None,
            until: None,
        };
        let mut count = 0usize;
        let _ = self.store.query_visit(&query, usize::MAX, &mut |_ev| {
            count += 1;
            ControlFlow::Continue(())
        });
        count
    }

    /// Relay URLs recorded in the durable store's provenance for `id_hex`
    /// (test-support). The de-facto provenance lens for codex-#11: a
    /// cache-served event carries no relay provenance until a live relay
    /// delivery records one, so the length of this list transitions from 0 to 1
    /// when a cache-served event is later confirmed by a relay.
    pub(crate) fn store_provenance_relays_for_test(&self, id_hex: &str) -> Vec<String> {
        let Some(id) = hex_to_id(id_hex) else {
            return Vec::new();
        };
        self.store
            .provenance_for(&id)
            .map(|rows| rows.into_iter().map(|r| r.relay_url).collect())
            .unwrap_or_default()
    }
}

/// Encode a 32-byte store id as a 64-char lowercase hex string.
fn hex32(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Decode a 64-char hex string to a 32-byte store id; `None` if malformed.
fn hex_to_id(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
