//! K3 Stage E (ADR-0056 §3) — coverage-ledger WRITE path on the kernel.
//!
//! Split out of `kernel/mod.rs` (LOC cap) as a cohesive owner: the two
//! completion entry points (EOSE for a plain REQ, NEG-DONE for a NIP-77
//! negentropy reconciliation), and the canonical-key extraction all live here.
//! The store-level row type and read/write primitives live in `nmp-store`
//! (`CoverageRow`, `EventStore::record_coverage` / `get_coverage`).
//!
//! Stage E: the presence-floor heuristic is deleted. The coverage ledger is
//! now the unconditional sole since-floor source. No row ⇒ full `[0, ∞)` window.

use std::sync::Arc;

use nmp_store::EventStore;

use super::Kernel;
use crate::planner::{canonical_filter_hash, InterestShape};
use crate::subs::WatermarkFn;

/// Build the kernel's installed since-floor resolver (`WatermarkFn`).
///
/// Extracted from `Kernel::new` so the constructor stays under the file-size
/// cap and the floor logic lives next to the ledger write path it reads.
///
/// The resolver is invoked per non-ephemeral sub-shape with the relay the REQ
/// targets; it returns the floor BASE (the `+1` is applied by the
/// floor-application sites `apply_watermark_rewrite` / `handle_reconnect`).
///
/// K3 Stage E: the coverage ledger is the **unconditional** sole floor source.
/// No presence scan; no flag; a `(canonical_filter_hash, relay)` row present ⇒
/// floor at `covered_through`; no row ⇒ refuse the floor (full `[0, ∞)` window).
pub(super) fn build_watermark_fn(store: Arc<dyn EventStore>) -> WatermarkFn {
    Arc::new(move |shape: &InterestShape, relay_url: &str| coverage_floor(&store, shape, relay_url))
}

/// K3 Stage E (ADR-0056 §3.E) — the coverage ledger is the **unconditional**
/// sole since-floor source. No presence fallback, no flag.
///
/// - **Ledger HAS a row** for `(canonical_filter_hash(shape), relay)` →
///   `Some(covered_through)`. Coverage is the sound floor source.
/// - **Ledger has NO row** (un-synced `(filter_hash, relay)`) → `None` —
///   **refuse to floor** (full `[0, ∞)` window). The H1 fix: an un-synced
///   `(filter_hash, relay)` fetches the full window; the relay's EOSE /
///   NEG-DONE records honest coverage; subsequent syncs floor at
///   `covered_through`.
pub(crate) fn coverage_floor(
    store: &Arc<dyn EventStore>,
    shape: &InterestShape,
    relay_url: &str,
) -> Option<u64> {
    let filter_hash = canonical_filter_hash(shape);
    // Coverage — not presence — is the floor authority.
    //   row present ⇒ floor at the honest completed-coverage bound;
    //   no row      ⇒ refuse to floor (full `[0, ∞)` window, the H1 fix).
    store.get_coverage(&filter_hash, relay_url)
}

impl Kernel {
    /// Record completed coverage at NEG-DONE.
    ///
    /// Called from the NIP-77 runtime (`nmp-nip77::runtime`) when a negentropy
    /// reconciliation reaches its terminal `Done` outcome for `(sub_id, relay)`.
    /// Per ADR-0056 Stage A the NEG reconciliation runs **un-floored** over the
    /// full `[0, ∞)` window, so a completed reconciliation honestly covers
    /// `[0, now]` — the downward-closed ledger is advanced to `now`
    /// unconditionally (no floor to guard against, unlike the plain-REQ EOSE
    /// path). Gated on the off-by-default flag inside `record_coverage_complete`.
    ///
    /// `now_secs` is threaded in by the caller (the NIP-77 runtime already reads
    /// `kernel.now_secs()` for its liveness deadline) so this method does not
    /// re-read the clock — a single clock read per terminal event.
    pub fn record_neg_done_coverage(&self, sub_id: &str, relay_url: &str, now_secs: u64) {
        self.record_coverage_complete(sub_id, relay_url, now_secs);
    }

    /// Record completed coverage at EOSE for a plain REQ.
    ///
    /// The relay has sent everything it has in the REQ window, so `[since_floor,
    /// now]` is covered. We advance the downward-closed ledger ONLY for an
    /// un-floored REQ (`since_floor` absent or `0`), which honestly proves
    /// `[0, now]`; a `since`-floored REQ proves only `[floor, now]`, so it
    /// records NO coverage rather than over-claim `[0, floor)` (the over-claim
    /// ADR-0056 §1 says makes presence unsound). Gated on the off-by-default
    /// flag inside `record_coverage_complete`.
    pub(crate) fn record_eose_coverage(
        &self,
        sub_id: &str,
        relay_url: &str,
        since_floor: Option<u64>,
        now_secs: u64,
    ) {
        let covered_through = match since_floor {
            None | Some(0) => now_secs,
            Some(_floor) => 0,
        };
        self.record_coverage_complete(sub_id, relay_url, covered_through);
    }

    /// Record completed coverage for a wire sub, keyed by the canonical filter
    /// hash extracted from `sub_id`.
    ///
    /// `sub_id` is the planner-assigned wire id (`sub-<canonical_filter_hash>`);
    /// the hash after the `sub-` prefix is the ledger key half so the floor read
    /// finds the row by the SAME key `recompile` builds.
    /// `covered_through` is the upper bound of the **downward-closed** window
    /// the completion proved `[0, covered_through]` — `0` is a no-op (no row).
    ///
    /// D6 graceful degrade: the store seam swallows write errors, so a failed
    /// ledger write never blocks the EOSE/NEG path.
    pub(crate) fn record_coverage_complete(
        &self,
        sub_id: &str,
        relay_url: &str,
        covered_through: u64,
    ) {
        // Only planner `sub-<hash>` ids carry a canonical filter hash; the legacy
        // `seed-timeline` / `diag-firehose-` / oneshot ids do not map to a
        // recompile floor key, so there is nothing the ledger could be read by.
        // Skip them rather than invent a non-canonical key.
        let Some(filter_hash) = sub_id.strip_prefix("sub-") else {
            return;
        };
        self.store
            .record_coverage(filter_hash, relay_url, covered_through);
    }
}
