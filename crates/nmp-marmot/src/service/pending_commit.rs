//! Pending-MLS-commit handles that enforce the mandatory commit/clear
//! discipline after a group-changing MDK call (mdk-api.md §7.7):
//! [`PendingGroupChange`] (returned by `add_members`/`remove_members`/
//! `self_update`/`leave_group`) and [`CreateGroupPending`] (returned by
//! `create_group`). Also owns the shared MLS group-id hex-encoding helper.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use mdk_core::prelude::GroupId;
use nostr::{Event, UnsignedEvent};

use super::{MarmotError, MarmotService, Result};

/// A group state change that produced an MLS pending commit which MUST be
/// resolved exactly once: [`commit`](Self::commit) on relay-publish success,
/// or [`clear`](Self::clear) on relay-publish failure (mdk-api.md §7.7).
///
/// `evolution_event` is the signed kind:445 event the caller publishes to the
/// group relay. `welcome_rumors` (if any) are kind:444 rumors the caller
/// gift-wraps (NIP-59) and delivers to invitees — use
/// [`MarmotService::wrap_welcome`].
#[must_use = "a PendingGroupChange must be commit()'d on publish-success or clear()'d on failure"]
pub struct PendingGroupChange<'a> {
    service: &'a MarmotService,
    group_id: GroupId,
    /// `true` for SelfRemove (`leave_group`): a peer commits it, so this
    /// handle's `commit()` is a no-op (NO `merge_pending_commit`).
    self_remove: bool,
    resolved: bool,
    /// Shared counter from the owning `MarmotService`. Incremented in
    /// `Drop` when the handle is dropped unresolved (V-61 diagnostic).
    orphaned_commit_count: Arc<AtomicU32>,
    pub evolution_event: Event,
    pub welcome_rumors: Vec<UnsignedEvent>,
}

impl<'a> PendingGroupChange<'a> {
    /// Construct a handle from an MDK `UpdateGroupResult`-derived triple.
    /// `pub(crate)` — only `MarmotService`'s orchestration methods build one.
    pub(crate) fn new(
        service: &'a MarmotService,
        group_id: GroupId,
        self_remove: bool,
        orphaned_commit_count: Arc<AtomicU32>,
        evolution_event: Event,
        welcome_rumors: Vec<UnsignedEvent>,
    ) -> Self {
        Self {
            service,
            group_id,
            self_remove,
            resolved: false,
            orphaned_commit_count,
            evolution_event,
            welcome_rumors,
        }
    }

    /// Call after the `evolution_event` was successfully published to the
    /// group relay. Performs `merge_pending_commit` (except SelfRemove).
    #[must_use]
    pub fn commit(mut self) -> Result<()> {
        self.resolved = true;
        if self.self_remove {
            // SelfRemove (leave_group): a peer auto-commits; we do NOT merge.
            return Ok(());
        }
        self.service
            .mdk
            .merge_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// Call if the `evolution_event` failed to publish. Clears the MLS
    /// pending commit so future group ops are not blocked (mdk-api.md §7.7).
    #[must_use]
    pub fn clear(mut self) -> Result<()> {
        self.resolved = true;
        if self.self_remove {
            // No pending commit was created for SelfRemove.
            return Ok(());
        }
        self.service
            .mdk
            .clear_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// The MLS group id this change applies to (hex).
    pub fn group_id_hex(&self) -> String {
        hex_encode(self.group_id.as_slice())
    }
}

impl<'a> Drop for PendingGroupChange<'a> {
    fn drop(&mut self) {
        // Defensive: if a caller drops the handle without resolving it (e.g.
        // a panic / early return), clear the pending commit so the group is
        // not wedged. A correct caller always commit()'s or clear()'s.
        if !self.resolved && !self.self_remove {
            let _ = self.service.mdk.clear_pending_commit(&self.group_id);
            // V-61: record the orphaned commit so the host can observe the
            // divergence. The pending commit was cleared (group is not wedged),
            // but the kind:445/commit event was never published — local MLS
            // state and the relay-published epoch may have diverged.
            let group_id_hex = hex_encode(self.group_id.as_slice());
            self.orphaned_commit_count.fetch_add(1, Ordering::Relaxed);
            // Surface the error as a typed `MarmotError::OrphanedCommit` via
            // stderr so it is never silently swallowed. The projection also
            // reads `orphaned_commit_count` and surfaces it in the snapshot.
            let err = MarmotError::OrphanedCommit { group_id_hex };
            eprintln!("nmp-marmot: {err}");
        }
    }
}

/// The pending-commit handle returned by [`MarmotService::create_group`].
/// `create_group` produces no evolution_event, so this is a distinct type
/// from [`PendingGroupChange`] (which carries one) but enforces the same
/// commit/clear discipline.
#[must_use = "a CreateGroupPending must be commit()'d on welcome-publish success or clear()'d on failure"]
pub struct CreateGroupPending<'a> {
    service: &'a MarmotService,
    group_id: GroupId,
    resolved: bool,
    /// Shared counter from the owning `MarmotService`. Incremented in
    /// `Drop` when the handle is dropped unresolved (V-61 diagnostic).
    orphaned_commit_count: Arc<AtomicU32>,
    pub welcome_rumors: Vec<UnsignedEvent>,
}

impl<'a> CreateGroupPending<'a> {
    /// Construct a handle for a freshly created group.
    /// `pub(crate)` — only `MarmotService::create_group` builds one.
    pub(crate) fn new(
        service: &'a MarmotService,
        group_id: GroupId,
        orphaned_commit_count: Arc<AtomicU32>,
        welcome_rumors: Vec<UnsignedEvent>,
    ) -> Self {
        Self {
            service,
            group_id,
            resolved: false,
            orphaned_commit_count,
            welcome_rumors,
        }
    }

    /// Call after the kind:444 welcome rumors were delivered. Performs the
    /// mandatory `merge_pending_commit` (mdk-api.md §7.3).
    #[must_use]
    pub fn commit(mut self) -> Result<()> {
        self.resolved = true;
        self.service
            .mdk
            .merge_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// Call if welcome delivery failed; clears the pending commit.
    #[must_use]
    pub fn clear(mut self) -> Result<()> {
        self.resolved = true;
        self.service
            .mdk
            .clear_pending_commit(&self.group_id)
            .map_err(MarmotError::from)
    }

    /// The created group's MLS id.
    #[must_use]
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    /// The created group's MLS id, hex-encoded.
    #[must_use]
    pub fn group_id_hex(&self) -> String {
        hex_encode(self.group_id.as_slice())
    }
}
impl<'a> Drop for CreateGroupPending<'a> {
    fn drop(&mut self) {
        if !self.resolved {
            let _ = self.service.mdk.clear_pending_commit(&self.group_id);
            // V-61: record the orphaned commit (see `PendingGroupChange::drop`).
            let group_id_hex = hex_encode(self.group_id.as_slice());
            self.orphaned_commit_count.fetch_add(1, Ordering::Relaxed);
            let err = MarmotError::OrphanedCommit { group_id_hex };
            eprintln!("nmp-marmot: {err}");
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
