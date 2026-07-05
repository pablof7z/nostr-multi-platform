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
use nmp_core::ObservedProjectionId;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    TeardownAction,
};

use crate::NmpApp;

// The ONE canonical `ReadHost` impl lives on the detached `NmpReadHost` handle
// (`read_host_handle.rs`) so a worker thread can open reads after an off-thread
// resolve (#2927). `NmpApp` delegates every method to a freshly-vended handle
// built from the same Arc-backed registry slots — one implementation, no fork.
impl ReadHost for NmpApp {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        self.read_host().install_read_output(key, encoder);
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.read_host().open_read_interest(decl)
    }

    fn open_live_only_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.read_host().open_live_only_read_interest(decl)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        self.read_host().teardown_close_interest(id)
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        self.read_host().teardown_remove_output(key)
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        self.read_host().teardown_mark_changed()
    }

    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.read_host().store_read_session(build)
    }

    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.read_host().read_session_projection_key(id)
    }

    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.read_host().close_read_session(id)
    }

    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.read_host().close_read_session_by_projection_key(projection_key)
    }

    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        self.read_host().read_interest_controller()
    }

    fn read_session_id_for_projection_key(
        &self,
        projection_key: &str,
    ) -> Option<nmp_read_session::ReadSessionId> {
        self.read_host()
            .read_session_id_for_projection_key(projection_key)
    }

    fn read_demand_set_members(
        &self,
        projection_key: &str,
    ) -> Option<nmp_read_session::DemandSetMembers> {
        self.read_host().read_demand_set_members(projection_key)
    }

    fn read_demand_set_reducer(
        &self,
        projection_key: &str,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        self.read_host().read_demand_set_reducer(projection_key)
    }
}
