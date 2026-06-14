//! ADR-0055 Rung 1 (F3) — `impl Kernel` block for the biconditional oracle.
//!
//! Lives in a sibling file (not `kernel/mod.rs`) so the already-at-baseline
//! `kernel/mod.rs` is not grown past its file-size baseline (AGENTS.md). The
//! whole module is `cfg(any(test, feature = "test-support"))` — a production
//! build neither compiles nor links it, so the emit path carries ZERO oracle
//! cost.

use crate::kernel::projection_rev::{self, oracle};
use crate::kernel::Kernel;
use crate::update_envelope::TypedProjectionData;

impl Kernel {
    /// Run the biconditional completeness oracle for the current emit and panic
    /// on any violation.
    ///
    /// `typed` is the FINAL typed sidecar `make_update` is about to serialize
    /// (post-`merge_builtin_typed_projections`). For each Tier-2 built-in we
    /// fingerprint the exact host cache unit and assert: if the cache unit
    /// changed since the previous emit, the rev MUST have advanced (or presence
    /// is Changed/Cleared) — otherwise the host serves a stale projection (a
    /// missed stamp). The drain keys' `Cleared` presence is set in
    /// `pending_presence` by `note_drain_emit`, so the manifest reflects the real
    /// tristate here.
    ///
    /// `record_tick` then folds this tick's fingerprints into `OracleState` and
    /// calls `record_emitted` for every key, advancing the tracker's last-emit
    /// baseline and clearing the per-tick `pending_presence` overrides.
    pub(crate) fn run_projection_oracle(&mut self, typed: &[TypedProjectionData]) {
        let manifest = self.projection_manifest();
        let violations =
            oracle::check_oracle(&self.projection_oracle.prev_fingerprints, &manifest, typed);
        assert!(
            violations.is_empty(),
            "ADR-0055 projection-rev oracle violation(s): {violations:?} — a projection's \
             cache unit changed but its rev did not advance (missed stamp = silent stale UI). \
             Add the missing source-version bump at the mutation's write chokepoint."
        );
        // Borrow-split: take the oracle out, fold the tick, put it back.
        let mut oracle = std::mem::take(&mut self.projection_oracle);
        oracle.record_tick(&manifest, typed, &mut self.projection_rev_tracker);
        self.projection_oracle = oracle;
    }

    /// The per-key `ProjectionState` as the MOST RECENT `make_update` actually
    /// emitted it (presence overrides applied, before the tracker's last-emit
    /// baseline advanced). Tests use this to assert the real
    /// `Changed` / `Cleared` / `Unchanged` tristate a tick carried — reading the
    /// live manifest after an emit would always report `Unchanged`.
    pub(crate) fn last_emitted_projection_state(
        &self,
        key: &str,
    ) -> Option<projection_rev::ProjectionState> {
        self.projection_oracle
            .last_emit_manifest
            .as_ref()
            .and_then(|m| m.states.iter().find(|s| s.key == key).cloned())
    }
}
