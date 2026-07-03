use nmp_nip29::group_id::GroupId;
use nmp_read_session::ReadHandle;

/// Descriptor for a group-scoped NIP-25 reaction-aggregate typed read session.
///
/// The reaction fold (kind:7) is scoped to one NIP-29 group: the session opens
/// a relay-pinned `#h` + `kinds:[5,7]` interest for `group_id` so only that
/// group's reactions feed the aggregate (and relay-delivered kind:5 deletions
/// decrement it). NIP-25 owns kind:7; the group scope is composed here at the
/// app layer.
///
/// `active_pubkey` is the viewer (active account, raw hex). The aggregate uses
/// it to surface the viewer's own kind:7 ids (`mine`) per target so the app can
/// retract (toggle-off) a reaction. An empty `active_pubkey` simply disables the
/// `mine` handles — the read-only aggregate is still produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip25GroupReactionsSession {
    pub(super) group_id: GroupId,
    pub(super) active_pubkey: String,
}

impl Nip25GroupReactionsSession {
    #[must_use]
    pub fn new(group_id: GroupId, active_pubkey: String) -> Self {
        Self {
            group_id,
            active_pubkey,
        }
    }

    #[must_use]
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    #[must_use]
    pub fn active_pubkey(&self) -> &str {
        &self.active_pubkey
    }
}

/// Runtime handle for one group-scoped NIP-25 reaction-aggregate read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip25GroupReactionsHandle {
    pub(super) read_handle: ReadHandle,
}

impl Nip25GroupReactionsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.read_handle.projection_key
    }
}
