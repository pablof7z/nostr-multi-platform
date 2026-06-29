use nmp_nip29::group_id::GroupId;

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
    pub(super) key: String,
    pub(super) handle_id: u64,
}

impl Nip29GroupEventsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Runtime handle for one host-driven NIP-29 group-discovery read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupDiscoveryHandle {
    pub(super) key: String,
    pub(super) handle_id: u64,
}

impl Nip29GroupDiscoveryHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Runtime handle for the active account's NIP-29 joined-groups read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29JoinedGroupsHandle {
    pub(super) key: String,
    pub(super) handle_id: u64,
}

impl Nip29JoinedGroupsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}
