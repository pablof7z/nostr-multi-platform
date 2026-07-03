use nmp_nip29::group_id::GroupId;
use nmp_read_session::ReadHandle;

/// Descriptor for a NIP-29 group-events typed read session.
///
/// The host relay pin is explicit because NIP-29 group reads are routed to the
/// group host relay. `kinds` is the consumer's kind selection: empty means all
/// h-tagged group events, while chat views usually pass `[9, 11]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupEventsSession {
    pub(super) group_id: GroupId,
    pub(super) kinds: Vec<u32>,
}

impl Nip29GroupEventsSession {
    #[must_use]
    pub fn new(group_id: GroupId, kinds: Vec<u32>) -> Self {
        Self { group_id, kinds }
    }

    #[must_use]
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    #[must_use]
    pub fn kinds(&self) -> &[u32] {
        &self.kinds
    }
}

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

/// Descriptor for a NIP-29 group-discovery typed read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupDiscoverySession {
    pub(super) host_relay_url: String,
}

impl Nip29GroupDiscoverySession {
    #[must_use]
    pub fn new(host_relay_url: String) -> Self {
        Self { host_relay_url }
    }

    #[must_use]
    pub fn host_relay_url(&self) -> &str {
        &self.host_relay_url
    }
}

/// Descriptor for a NIP-29 single-group member-roster typed read session.
///
/// Scoped to one group `(host_relay_url, local_id)`. The session subscribes to
/// that group's relay-signed 39001 (admins) / 39002 (members) / 39003 (roles)
/// snapshots and exposes the full roster — member pubkeys + per-member role
/// tokens + the group's role catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupRosterSession {
    pub(super) group_id: GroupId,
}

impl Nip29GroupRosterSession {
    #[must_use]
    pub fn new(group_id: GroupId) -> Self {
        Self { group_id }
    }

    #[must_use]
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }
}

/// Descriptor for the active account's NIP-29 joined-groups typed read session.
///
/// If `host_relay_url` is empty, relay provenance decides each group's host and
/// the interest is outbox-routed. Otherwise the session is pinned to that host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29JoinedGroupsSession {
    pub(super) active_pubkey: String,
    pub(super) host_relay_url: String,
}

impl Nip29JoinedGroupsSession {
    #[must_use]
    pub fn new(active_pubkey: String, host_relay_url: String) -> Self {
        Self {
            active_pubkey,
            host_relay_url,
        }
    }

    #[must_use]
    pub fn active_pubkey(&self) -> &str {
        &self.active_pubkey
    }

    #[must_use]
    pub fn host_relay_url(&self) -> &str {
        &self.host_relay_url
    }
}

/// Runtime handle for one host-driven NIP-29 group-events read session.
///
/// The handle carries only the session key. It never stores an app pointer; the
/// caller closes it by passing the handle back to the owning NMP app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupEventsHandle {
    pub(super) read_handle: ReadHandle,
}

impl Nip29GroupEventsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.read_handle.projection_key
    }
}

/// Runtime handle for one host-driven NIP-29 group-discovery read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupDiscoveryHandle {
    pub(super) read_handle: ReadHandle,
}

impl Nip29GroupDiscoveryHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.read_handle.projection_key
    }
}

/// Runtime handle for the active account's NIP-29 joined-groups read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29JoinedGroupsHandle {
    pub(super) read_handle: ReadHandle,
}

impl Nip29JoinedGroupsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.read_handle.projection_key
    }
}

/// Runtime handle for one host-driven NIP-29 group-roster read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupRosterHandle {
    pub(super) read_handle: ReadHandle,
}

impl Nip29GroupRosterHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.read_handle.projection_key
    }
}
