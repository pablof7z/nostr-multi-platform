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

use nmp_core::__ffi_internal::SnapshotProjectionSlot;
use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ObservedProjection, ObservedProjectionCommandHandle};
use nmp_core::{CommandSendStatus, CommandSender, CompositionLedger, ObservedProjectionId};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    TeardownAction,
};

use crate::read_output_commands::{InstallReadOutputCommand, RemoveReadOutputCommand};
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
        // #3080 — deferred, not synchronous. A snapshot-projection closure can
        // legally capture an `Arc<NmpApp>` / `NmpReadHost` and call
        // `open_read` from inside the snapshot tick; the old body here
        // re-locked `snapshot_projections` on the caller's thread, which is
        // the SAME lock the tick holds while running closures pre-#3079 (and
        // still re-locks other synchronous introspection paths). Enqueuing a
        // `Protocol` command instead means this door never re-locks on the
        // caller's thread, regardless of who the caller is — re-entrancy is
        // gone by construction, not by the emit loop's timing discipline.
        // Best-effort, matching the observed-interest half of the same open
        // (`self.observed.open`, a fire-and-forget `try_send`): a dropped
        // install under a saturated inbox loses one read output rather than
        // blocking the caller.
        let status =
            self.command_sender
                .send(ActorCommand::Protocol(Box::new(InstallReadOutputCommand {
                    key,
                    producer: encoder,
                    projections: Arc::clone(&self.snapshot_projections),
                    ledger: Arc::clone(&self.composition_ledger),
                })));
        if matches!(status, Ok(CommandSendStatus::DroppedFull)) {
            tracing::warn!("install_read_output dropped — actor inbox full");
        }
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
        // #3080 — same deferral as `install_read_output`: enqueue a `Protocol`
        // command instead of re-locking `snapshot_projections` when this
        // teardown closure runs (which may itself be from inside a snapshot
        // closure that is tearing down and reopening a session mid-tick).
        let projections = Arc::clone(&self.snapshot_projections);
        let sender = self.command_sender.clone();
        Box::new(move || {
            let status = sender.send(ActorCommand::Protocol(Box::new(RemoveReadOutputCommand {
                key,
                projections,
            })));
            if matches!(status, Ok(CommandSendStatus::DroppedFull)) {
                tracing::warn!("teardown_remove_output dropped — actor inbox full");
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

    fn read_demand_set_reconciler(
        &self,
        projection_key: &str,
    ) -> Option<std::sync::Arc<nmp_read_session::DemandSetReconciler>> {
        self.read_sessions
            .as_read_sessions()
            .demand_set_reconciler(projection_key)
    }
}

#[cfg(test)]
#[path = "read_host_handle_tests.rs"]
mod tests;
