//! `NmpApp`'s implementation of the concept-neutral read-lifecycle host seam
//! (`nmp_read_session::ReadHost`, #2777).
//!
//! This is the ONE, GENERIC place the native runtime wires the read-lifecycle
//! mechanics — install a typed output, open a replay-before-live observed
//! interest, record the reverse-teardown steps, and share the ONE read-session
//! registry (`feed_sessions.as_read_sessions()`). It grows NO per-concept
//! method and NO per-concept dependency: a concept crate (e.g. `nmp-replies`)
//! defines its own door (`open_replies`) and drives it through this seam, so a
//! kernel that never imports that concept crate has none of its symbols. A
//! browser runtime implements the same seam once to get parity — no
//! concept-by-concept porting.
//!
//! Doctrine map:
//! - D0: this seam names no protocol/concept noun; it moves only opaque
//!   filters, observers, keys, and teardown closures.
//! - D4: the lifecycle registry is the shared engine registry, not a second
//!   one; teardown reuses the existing observed-projection close / snapshot
//!   removal / mark-changed paths.
//! - D8: every interest opened is withdrawn and the output tombstoned on close,
//!   in reverse order; no polling.

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, TeardownAction};

use crate::NmpApp;

impl ReadHost for NmpApp {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        // Coalesced typed emission (ADR-0070 revision ladder / ADR-0072) is owned by the snapshot
        // registry; the concept only supplies the per-tick encoder.
        self.register_typed_snapshot_projection(key, move || -> Option<TypedProjectionData> {
            encoder()
        });
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        // Replay-before-live + live activation + exact withdrawal are one kernel
        // primitive; the concept never sequences them by hand.
        self.observed_projection_handle().open(decl)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let handle = self.observed_projection_handle();
        Box::new(move || {
            use nmp_core::substrate::ObservedProjectionRegistrar;
            handle.close_observed_projection(id);
        })
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        let projections = self.snapshot_projections_handle();
        Box::new(move || {
            if let Ok(mut registry) = projections.lock() {
                let _ = registry.remove(&key);
            }
        })
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        let sender = self.command_sender();
        Box::new(move || {
            sender.mark_changed_since_emit();
        })
    }

    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.feed_sessions.as_read_sessions().open(build)
    }

    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.feed_sessions.as_read_sessions().projection_key(id)
    }

    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.feed_sessions.as_read_sessions().close(id)
    }
}
