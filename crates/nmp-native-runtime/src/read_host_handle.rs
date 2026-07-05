//! `NmpReadHost` — a `Send + Sync + 'static` clone of the read-lifecycle host
//! seam (`nmp_read_session::ReadHost`), detached from a `&NmpApp` borrow.
//!
//! `NmpApp`'s own `ReadHost` impl (`app_impl_read_host.rs`) is `&self`-bound, so
//! it can only open reads on a thread that holds the app. NIP-AD moment-1
//! (render) and moment-2 (paste/search) both need to open a relay-pinned
//! collection AFTER an off-thread `.well-known` resolve (#2927), on a worker
//! thread that cannot borrow `NmpApp`. `NmpReadHost` captures exactly the same
//! Arc-backed registry slots `NmpApp` uses and carries the ONE canonical
//! `ReadHost` impl; `NmpApp` delegates to a freshly-built handle so there is a
//! single implementation, no fork.

use std::sync::Arc;

use nmp_core::substrate::{ObservedProjection, ObservedProjectionCommandHandle};
use nmp_core::{CommandSender, CompositionLedger, ObservedProjectionId};
use nmp_core::__ffi_internal::SnapshotProjectionSlot;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    TeardownAction,
};

use crate::snapshot::register_typed_snapshot_projection_on;
use crate::NmpApp;

/// Detached, cheaply-cloneable read-lifecycle host. Every field is an Arc-backed
/// handle shared with the owning [`NmpApp`], so opening/closing reads through
/// this handle is identical to doing so through `&NmpApp`.
#[derive(Clone)]
pub struct NmpReadHost {
    observed: ObservedProjectionCommandHandle,
    snapshot_projections: SnapshotProjectionSlot,
    composition_ledger: Arc<CompositionLedger>,
    command_sender: CommandSender,
    read_sessions: Arc<nmp_feed::FeedSessionRegistry>,
}

impl NmpApp {
    /// Vend a `Send + Sync + 'static` [`NmpReadHost`] that opens reads against
    /// the same registries as `&self`. Used to hand a read host to a worker
    /// thread (NIP-AD off-thread resolve → `open_ad_collection`, #2927).
    #[must_use]
    pub(crate) fn read_host(&self) -> NmpReadHost {
        NmpReadHost {
            observed: self.observed_projection_handle(),
            snapshot_projections: Arc::clone(&self.snapshot_projections),
            composition_ledger: Arc::clone(&self.composition_ledger),
            command_sender: self.command_sender(),
            read_sessions: Arc::clone(&self.feed_sessions),
        }
    }
}

impl ReadHost for NmpReadHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        // Same coalesced typed emission NmpApp registers (ADR-0069/0070/0072),
        // via the one canonical registration body — no fork.
        register_typed_snapshot_projection_on(
            &self.snapshot_projections,
            &self.composition_ledger,
            key,
            move |_tick| encoder(),
        );
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed.open(decl)
    }

    fn open_live_only_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed.open_live_only(decl)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let handle = self.observed.clone();
        Box::new(move || {
            use nmp_core::substrate::ObservedProjectionRegistrar;
            handle.close_observed_projection(id);
        })
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        let projections = Arc::clone(&self.snapshot_projections);
        Box::new(move || {
            if let Ok(mut registry) = projections.lock() {
                let _ = registry.remove(&key);
            }
        })
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        let sender = self.command_sender.clone();
        Box::new(move || {
            sender.mark_changed_since_emit();
        })
    }

    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.read_sessions.as_read_sessions().open(build)
    }

    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.read_sessions.as_read_sessions().projection_key(id)
    }

    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.read_sessions.as_read_sessions().close(id)
    }

    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.read_sessions
            .as_read_sessions()
            .close_by_projection_key(projection_key)
    }

    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        let opener = self.observed.clone();
        let closer = opener.clone();
        Some(ReadInterestController::new(
            move |decl| opener.open(decl),
            move |id| closer.close(id),
        ))
    }

    fn read_session_id_for_projection_key(&self, projection_key: &str) -> Option<ReadSessionId> {
        self.read_sessions
            .as_read_sessions()
            .session_id_for_projection_key(projection_key)
    }

    fn read_demand_set_members(
        &self,
        projection_key: &str,
    ) -> Option<nmp_read_session::DemandSetMembers> {
        self.read_sessions
            .as_read_sessions()
            .demand_set_members(projection_key)
    }

    fn read_demand_set_reducer(
        &self,
        projection_key: &str,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        self.read_sessions
            .as_read_sessions()
            .demand_set_reducer(projection_key)
    }
}
