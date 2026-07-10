//! Browser implementation of the concept-neutral read-lifecycle host seam.

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, TeardownAction,
};

use super::handle::BrowserRuntimeHandle;

impl ReadHost for BrowserRuntimeHandle {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        self.runtime
            .reducer
            .register_typed_snapshot_projection(key, move || -> Option<TypedProjectionData> {
                encoder()
            });
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed_projection_registrar.open(decl)
    }

    fn open_live_only_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed_projection_registrar.open_live_only(decl)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let registrar = self.observed_projection_registrar.clone();
        Box::new(move || registrar.close(id))
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        self.runtime.reducer.remove_snapshot_projection_action(key)
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        let sender = self.command_sender();
        Box::new(move || sender.mark_changed_since_emit())
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

    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.feed_sessions
            .as_read_sessions()
            .close_by_projection_key(projection_key)
    }

    fn read_session_id_for_projection_key(&self, projection_key: &str) -> Option<ReadSessionId> {
        self.feed_sessions
            .as_read_sessions()
            .session_id_for_projection_key(projection_key)
    }

    fn read_demand_set_members(
        &self,
        projection_key: &str,
    ) -> Option<nmp_read_session::DemandSetMembers> {
        self.feed_sessions
            .as_read_sessions()
            .demand_set_members(projection_key)
    }

    fn read_demand_set_reducer(
        &self,
        projection_key: &str,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        self.feed_sessions
            .as_read_sessions()
            .demand_set_reducer(projection_key)
    }

    fn read_demand_set_reconciler(
        &self,
        projection_key: &str,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        self.feed_sessions
            .as_read_sessions()
            .demand_set_reconciler(projection_key)
    }
}
