//! Deferred read-session output install/remove — the structural half of
//! #3080 (`nmp_read_session::ReadHost::install_read_output` /
//! `teardown_remove_output`).
//!
//! #3079 stopped the emit loop from RUNNING snapshot-projection closures while
//! holding the registry lock, so a closure that re-locks the registry (by
//! opening/closing a read-session) lands cleanly on the next tick instead of
//! deadlocking. That fix is safe by timing. This module removes the
//! synchronous re-lock entirely: `install_read_output` /
//! `teardown_remove_output` (`read_host_handle.rs`) no longer call
//! `snapshot_projections.lock()` on the calling thread — they enqueue one of
//! these two [`nmp_core::substrate::ProtocolCommand`]s instead, and the actor
//! applies the registry mutation on its own command turn, outside any
//! snapshot-tick lock scope. Re-entrancy from a snapshot closure is then
//! impossible by construction: there is no synchronous door left to re-enter.
//!
//! Both commands ignore [`ProtocolCommandContext`] entirely — they carry
//! everything they need (the captured `Arc` slots) at enqueue time and call
//! the same canonical [`register_typed_snapshot_projection_on`] /
//! `SnapshotRegistry::remove` bodies the synchronous path used, so there is
//! still exactly one registration/removal implementation, no fork.

use std::fmt;
use std::sync::Arc;

use nmp_core::__ffi_internal::SnapshotProjectionSlot;
use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_core::CompositionLedger;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::ReadOutputEncoder;

use crate::snapshot::register_typed_snapshot_projection_on;

/// Deferred-apply install of a read-session typed output.
///
/// Enqueued by `NmpReadHost::install_read_output` instead of registering
/// synchronously. `run` applies the SAME registration body the synchronous
/// path used (`register_typed_snapshot_projection_on`) — the only change is
/// WHEN it runs: on the actor's command turn, never on the caller's thread.
pub(crate) struct InstallReadOutputCommand {
    pub(crate) key: ProjectionRegistrationKey,
    pub(crate) producer: ReadOutputEncoder,
    pub(crate) projections: SnapshotProjectionSlot,
    pub(crate) ledger: Arc<CompositionLedger>,
}

// Manual `Debug`: `producer` is a boxed closure and `projections` is an
// `Arc<Mutex<SnapshotRegistry>>` (`SnapshotRegistry` is not `Debug`), so this
// cannot be derived. `ActorCommand` derives `Debug` and forwards through the
// boxed `ProtocolCommand` trait object, so a manual impl is required.
impl fmt::Debug for InstallReadOutputCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstallReadOutputCommand")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for InstallReadOutputCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            key,
            producer,
            projections,
            ledger,
        } = *self;
        register_typed_snapshot_projection_on(&projections, &ledger, key, move |_tick| producer());
        Ok(())
    }
}

/// Deferred-apply removal of a read-session typed output.
///
/// Enqueued by `NmpReadHost::teardown_remove_output`'s [`TeardownAction`]
/// (`nmp_read_session::TeardownAction`) instead of re-locking the registry on
/// the caller's thread when the teardown closure runs.
pub(crate) struct RemoveReadOutputCommand {
    pub(crate) key: String,
    pub(crate) projections: SnapshotProjectionSlot,
}

// Manual `Debug` for the same reason as `InstallReadOutputCommand` —
// `SnapshotRegistry` (behind `projections`) is not `Debug`.
impl fmt::Debug for RemoveReadOutputCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoveReadOutputCommand")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for RemoveReadOutputCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        if let Ok(mut registry) = self.projections.lock() {
            let _ = registry.remove(&self.key);
        }
        Ok(())
    }
}
