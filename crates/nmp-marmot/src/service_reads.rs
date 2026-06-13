//! Read projections for [`MarmotService`].
//!
//! These methods back the `domain` / `view` modules and the post-restart
//! `resubscribe_all_groups` cache re-seed. They are pure SQLite reads driving
//! `MDK` — no mutation, no signing. Split out of `service.rs` to keep that
//! file under the size cap; same `impl MarmotService`, same public API.

use std::collections::{BTreeMap, BTreeSet};

use mdk_core::prelude::{group_types, message_types, GroupId};
use nostr::{PublicKey, RelayUrl};

use crate::service::{MarmotError, MarmotService, Result};

impl MarmotService {
    /// All groups (any state). Backs `GroupList`.
    #[must_use]
    pub fn get_groups(&self) -> Result<Vec<group_types::Group>> {
        self.mdk.get_groups().map_err(MarmotError::from)
    }

    /// A single group's display metadata. Backs `MarmotGroup`.
    #[must_use]
    pub fn get_group(&self, group_id: &GroupId) -> Result<Option<group_types::Group>> {
        self.mdk.get_group(group_id).map_err(MarmotError::from)
    }

    /// The current member set (Nostr pubkeys). Backs `MarmotGroupRow.members`.
    #[must_use]
    pub fn get_members(&self, group_id: &GroupId) -> Result<BTreeSet<PublicKey>> {
        self.mdk.get_members(group_id).map_err(MarmotError::from)
    }

    /// Persisted relay-pinned relay URLs for a group (empty if never written).
    /// Backs the post-restart `resubscribe_all_groups` cache re-seed.
    #[must_use]
    pub fn group_relays(&self, group_id: &GroupId) -> Result<Vec<RelayUrl>> {
        self.mdk
            .get_relays(group_id)
            .map(|s| s.into_iter().collect())
            .map_err(MarmotError::from)
    }

    /// MLS leaf-index → pubkey map. Backs `MarmotGroupRow.members` leaf indices.
    pub fn group_leaf_map(&self, group_id: &GroupId) -> Result<BTreeMap<u32, PublicKey>> {
        self.mdk.group_leaf_map(group_id).map_err(MarmotError::from)
    }

    /// Decrypted message history (unpaginated). Backs `GroupMessages`.
    #[must_use]
    pub fn get_messages(&self, group_id: &GroupId) -> Result<Vec<message_types::Message>> {
        self.mdk
            .get_messages(group_id, None)
            .map_err(MarmotError::from)
    }

    /// Groups whose self-update (key rotation) is overdue past `threshold_secs`.
    /// Drives the TTL re-publish path (plan §Step 3).
    #[must_use]
    pub fn groups_needing_self_update(&self, threshold_secs: u64) -> Result<Vec<GroupId>> {
        self.mdk
            .groups_needing_self_update(threshold_secs)
            .map_err(MarmotError::from)
    }
}
