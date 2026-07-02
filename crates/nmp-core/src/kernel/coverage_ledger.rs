//! K3 coverage ledger (ADR-0072 §3) — the kernel's since-floor source.
//!
//! Split out of `kernel/mod.rs` (LOC cap) as a cohesive owner: the two
//! completion entry points (EOSE for a plain REQ, NEG-DONE for a NIP-77
//! negentropy reconciliation), the canonical-key extraction, and the since-floor
//! read all live here. The store-level row type and read/write primitives live
//! in `nmp-store` (`CoverageRow`, `EventStore::record_coverage` / `get_coverage`).
//!
//! As of Stage E the coverage ledger is the SOLE since-floor source: the
//! presence heuristic (and the off-by-default flag that gated the migration)
//! are gone. The floor for `(canonical_filter_hash(shape), relay)` is the
//! ledger's `covered_through`, or `None` (refuse the floor / full `[0, ∞)`
//! window) when the relay has no completed-coverage row.

use std::sync::Arc;

use nmp_store::EventStore;

use super::Kernel;
use crate::planner::{canonical_filter_hash, InterestShape};
use crate::subs::WatermarkFn;

/// Build the kernel's installed since-floor resolver (`WatermarkFn`) — reads the
/// coverage ledger for `(canonical_filter_hash(shape), relay)`. Extracted from
/// `Kernel::new` so the constructor stays under the file-size cap and the floor
/// read lives next to the ledger write path it reads.
///
/// Captured state: `store` (the ledger reads off this handle). The resolver is
/// invoked per non-ephemeral sub-shape with the relay the REQ targets; it
/// returns the floor BASE (the `+1` is applied by the floor-application sites
/// `apply_watermark_rewrite` / `handle_reconnect`).
pub(super) fn build_watermark_fn(store: Arc<dyn EventStore>) -> WatermarkFn {
    Arc::new(move |shape: &InterestShape, relay_url: &str| {
        // The coverage ledger is the floor source (the same decision the `&self`
        // wrapper `Kernel::coverage_floor_for`, which the unit tests drive,
        // makes — one definition, two call sites).
        coverage_floor(&store, shape, relay_url)
    })
}

/// K3 Stage D2 (ADR-0072 §3.D2) — the since-floor read-swap decision table, as
/// a free function so the SINGLE definition is shared by both floor-read sites:
///
/// 1. the installed [`crate::subs::WatermarkFn`] closure (`kernel/mod.rs`),
///    which cannot hold `&Kernel` (it is owned by the lifecycle, a sibling of
///    the kernel), so it captures the store `Arc` and calls this; and
/// 2. [`Kernel::coverage_floor_for`] (below), the `&self` wrapper the unit
///    tests drive directly.
///
/// One definition, two call sites — no second hand-synced copy to drift.
///
/// Decision:
///
/// - **Ledger HAS a row** for `(canonical_filter_hash(shape), relay)` →
///   `Some(covered_through)`. Coverage is the sound floor source: the relay has
///   completed a sync through `covered_through`, so a REQ may honestly floor
///   `since` to `covered_through + 1` (the `+1` is applied by the
///   floor-application sites `apply_watermark_rewrite` / `handle_reconnect`).
/// - **Ledger has NO row** (un-synced `(filter_hash, relay)`) → `None` —
///   **refuse to floor** (ADR-0072 §3.D2 item 2: "no completed-coverage row ⇒
///   refuse to floor"). The REQ runs un-floored over the full `[0, ∞)` window.
///
/// # Why no-row REFUSES the floor (rather than guessing from store presence)
///
/// The whole premise of the ledger (ADR-0072 §1) is *"presence is not
/// coverage."* The H1 headline is precisely the case where store presence is
/// unsound: a single stray event by author A (a thread reply stored under an
/// Etag shape) makes a presence floor for A's follow-feed shape `Some(stray_ts)`,
/// which would suppress A's history below the stray — *even though that shape
/// has never completed a sync against this relay*. The coverage ledger refuses
/// the floor for that un-synced shape, so the relay re-sends the full window,
/// the EOSE / NEG-DONE then records honest coverage, and *subsequent* syncs
/// floor at the recorded `covered_through`. A full backfill is never WORSE than
/// a floor — it can only fetch MORE, never suppress.
pub(crate) fn coverage_floor(
    store: &Arc<dyn EventStore>,
    shape: &InterestShape,
    relay_url: &str,
) -> Option<u64> {
    let filter_hash = canonical_filter_hash(shape);
    // Coverage — not store presence — is the floor authority.
    //   row present ⇒ floor at the honest completed-coverage bound;
    //   no row      ⇒ refuse to floor (full `[0, ∞)` window, the H1 fix).
    store.get_coverage(&filter_hash, relay_url)
}

impl Kernel {
    /// Resolve the since-floor base for `(shape, relay)` by reading the coverage
    /// ledger.
    ///
    /// Thin `&self` wrapper over [`coverage_floor`] (the shared definition) for
    /// callers that hold a `&Kernel` — chiefly the unit tests, which drive the
    /// read directly without standing up the full recompile/WireFrame path. The
    /// installed `WatermarkFn` closure calls the same free function with the same
    /// store handle, so the two floor-read sites are guaranteed identical.
    ///
    /// Test-only: production reads the floor through the installed `WatermarkFn`
    /// closure, which calls the free function directly.
    #[cfg(test)]
    pub(crate) fn coverage_floor_for(&self, shape: &InterestShape, relay_url: &str) -> Option<u64> {
        coverage_floor(&self.store, shape, relay_url)
    }

    /// Record completed coverage at NEG-DONE.
    ///
    /// Called from the NIP-77 runtime (`nmp-nip77::runtime`) when a negentropy
    /// reconciliation reaches its terminal `Done` outcome for `(sub_id, relay)`.
    /// Per ADR-0072 Stage A the NEG reconciliation runs **un-floored** over the
    /// full `[0, ∞)` window, so a completed reconciliation honestly covers
    /// `[0, now]` — the downward-closed ledger is advanced to `now`
    /// unconditionally (no floor to guard against, unlike the plain-REQ EOSE
    /// path).
    ///
    /// `now_secs` is threaded in by the caller (the NIP-77 runtime already reads
    /// `kernel.now_secs()` for its liveness deadline) so this method does not
    /// re-read the clock — a single clock read per terminal event.
    ///
    /// Also bumps the lifecycle's `watermark_generation` so the compile-input
    /// fingerprint in `recompile_and_diff_with_lookup` reflects the new coverage
    /// floor on the next triggered recompile (CRITICAL: prevents stale `since`
    /// values from causing silent under-fetch after negentropy reconciliation).
    pub fn record_neg_done_coverage(&mut self, sub_id: &str, relay_url: &str, now_secs: u64) {
        self.record_coverage_complete(sub_id, relay_url, now_secs);
        // Only planner `sub-<hash>` ids advance the ledger (see
        // `record_coverage_complete`). Only bump when the ledger actually changes.
        if sub_id.starts_with("sub-") {
            self.lifecycle.bump_watermark_generation();
        }
    }

    /// Record completed coverage at EOSE for a plain REQ.
    ///
    /// The relay has sent everything it has in the REQ window, so `[since_floor,
    /// now]` is covered. We advance the downward-closed ledger ONLY for an
    /// un-floored REQ (`since_floor` absent or `0`), which honestly proves
    /// `[0, now]`; a `since`-floored REQ proves only `[floor, now]`, so it
    /// records NO coverage rather than over-claim `[0, floor)` (the over-claim
    /// ADR-0072 §1 says makes presence unsound).
    ///
    /// Also bumps the lifecycle's `watermark_generation` when coverage advances
    /// (un-floored planner sub) so the compile-input fingerprint in
    /// `recompile_and_diff_with_lookup` reflects the new floor on the next
    /// triggered recompile (CRITICAL: prevents stale `since` → silent under-fetch).
    pub(crate) fn record_eose_coverage(
        &mut self,
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
        // Only un-floored EOSEs on planner sub ids advance the ledger.
        if covered_through > 0 && sub_id.starts_with("sub-") {
            self.lifecycle.bump_watermark_generation();
        }
    }

    /// Record completed coverage for a wire sub, keyed by the canonical filter
    /// hash extracted from `sub_id`.
    ///
    /// `sub_id` is the planner-assigned wire id (`sub-<canonical_filter_hash>`);
    /// the hash after the `sub-` prefix is the ledger key half so the Stage D2
    /// read swap finds the row by the SAME key `recompile` builds.
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
        // recompile floor key, so there is nothing the ledger could be read by in
        // D2. Skip them rather than invent a non-canonical key.
        let Some(filter_hash) = sub_id.strip_prefix("sub-") else {
            return;
        };
        self.store
            .record_coverage(filter_hash, relay_url, covered_through);
    }
}
