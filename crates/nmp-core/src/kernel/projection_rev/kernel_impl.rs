//! ADR-0055 Rung 1 — `impl Kernel` accessors for the projection-rev manifest.
//!
//! Lives in a sibling file (not `kernel/mod.rs`) so the already-at-baseline
//! `kernel/mod.rs` is not grown past its file-size baseline (AGENTS.md). These
//! are production (non-test) accessors — internal-only in Rung 1 (`make_update`
//! does NOT consult them; wire bytes stay byte-identical). Rung 2 stamps the
//! manifest onto the wire; Rung 3 uses it to omit Unchanged projections.

use crate::kernel::projection_rev;
use crate::kernel::Kernel;

impl Kernel {
    /// Return the full per-projection revision manifest for the current tick.
    ///
    /// `session_id` = `TimingMilestones::started_unix_ms` (ADR-0055 D4).
    pub(crate) fn projection_manifest(&self) -> projection_rev::ProjectionManifest {
        let session_id = self.timing.started_unix_ms.unwrap_or(0);
        projection_rev::build_manifest(&self.projection_rev_tracker, session_id)
    }

    /// Return the revision state for a single projection key.
    /// Returns `Unchanged` at rev 0 for an unknown key.
    pub(crate) fn projection_state(&self, key: &str) -> projection_rev::ProjectionState {
        projection_rev::build_state(&self.projection_rev_tracker, key)
    }
}
