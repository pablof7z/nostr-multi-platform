//! K3 Stage D1 (ADR-0056 §3) — coverage-ledger WRITE path on the kernel.
//!
//! Split out of `kernel/mod.rs` (LOC cap) as a cohesive owner: the flag, the
//! two completion entry points (EOSE for a plain REQ, NEG-DONE for a NIP-77
//! negentropy reconciliation), and the canonical-key extraction all live here.
//! The store-level row type and read/write primitives live in `nmp-store`
//! (`CoverageRow`, `EventStore::record_coverage` / `get_coverage`).
//!
//! D1 is WRITE-only and OFF by default: with `coverage_ledger_enabled == false`
//! nothing is recorded and nothing reads the ledger; the since-floor stays
//! presence-derived until the Stage D2 read swap.

use super::Kernel;

impl Kernel {
    /// Enable/disable the coverage-ledger WRITE path. Default `false`.
    ///
    /// With the flag off the kernel records no coverage at EOSE / NEG-DONE (a
    /// pure no-op); with it on the ledger fills, but READ behaviour is unchanged
    /// in D1 (the since-floor is swapped to read the ledger only in Stage D2).
    pub fn set_coverage_ledger_enabled(&mut self, enabled: bool) {
        self.coverage_ledger_enabled = enabled;
    }

    /// Whether the coverage-ledger write path is enabled.
    #[must_use]
    pub fn coverage_ledger_enabled(&self) -> bool {
        self.coverage_ledger_enabled
    }

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
    /// the hash after the `sub-` prefix is the ledger key half so the Stage D2
    /// read swap finds the row by the SAME key `recompile` builds.
    /// `covered_through` is the upper bound of the **downward-closed** window
    /// the completion proved `[0, covered_through]` — `0` is a no-op (no row).
    ///
    /// Gated on the off-by-default flag: with the flag off this is a no-op and
    /// no row is ever written. D6 graceful degrade: the store seam swallows
    /// write errors, so a failed ledger write never blocks the EOSE/NEG path.
    pub(crate) fn record_coverage_complete(
        &self,
        sub_id: &str,
        relay_url: &str,
        covered_through: u64,
    ) {
        if !self.coverage_ledger_enabled {
            return;
        }
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
