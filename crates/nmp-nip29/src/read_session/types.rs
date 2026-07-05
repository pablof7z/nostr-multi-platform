use nmp_read_session::ReadHandle;

use crate::group_id::GroupId;

/// Descriptor for a NIP-29 group-events typed read session.
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
///
/// `host_relay_urls` is the FULL desired relay set (#93 multi-relay group
/// discovery) — not a delta. Re-opening with an updated set reconciles the
/// live session's membership (adds newly-named relays, withdraws relays no
/// longer named) rather than tearing the whole session down; see
/// [`super::open_nip29_group_discovery_session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupDiscoverySession {
    pub(super) host_relay_urls: Vec<String>,
}

impl Nip29GroupDiscoverySession {
    #[must_use]
    pub fn new(host_relay_urls: Vec<String>) -> Self {
        Self { host_relay_urls }
    }

    #[must_use]
    pub fn host_relay_urls(&self) -> &[String] {
        &self.host_relay_urls
    }
}

/// Descriptor for a NIP-29 single-group member-roster typed read session.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29JoinedGroupsSession {
    pub(super) active_pubkey: String,
    pub(super) host_relay_urls: Vec<String>,
}

impl Nip29JoinedGroupsSession {
    #[must_use]
    pub fn new(active_pubkey: String, host_relay_url: String) -> Self {
        Self::new_for_relays(active_pubkey, vec![host_relay_url])
    }

    #[must_use]
    pub fn new_for_relays(active_pubkey: String, host_relay_urls: Vec<String>) -> Self {
        Self {
            active_pubkey,
            host_relay_urls: normalize_relay_urls(host_relay_urls),
        }
    }

    #[must_use]
    pub fn active_pubkey(&self) -> &str {
        &self.active_pubkey
    }

    #[must_use]
    pub fn host_relay_url(&self) -> &str {
        self.host_relay_urls.first().map_or("", String::as_str)
    }

    #[must_use]
    pub fn host_relay_urls(&self) -> &[String] {
        &self.host_relay_urls
    }
}

fn normalize_relay_urls(host_relay_urls: Vec<String>) -> Vec<String> {
    let mut relays = host_relay_urls
        .into_iter()
        .filter(|relay| !relay.is_empty())
        .collect::<Vec<_>>();
    relays.sort();
    relays.dedup();
    relays
}

/// Runtime handle for one NIP-29 group-events read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupEventsHandle(pub(super) ReadHandle);

impl Nip29GroupEventsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Runtime handle for one NIP-29 group-discovery read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupDiscoveryHandle(pub(super) ReadHandle);

impl Nip29GroupDiscoveryHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Runtime handle for the active account's NIP-29 joined-groups read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29JoinedGroupsHandle(pub(super) ReadHandle);

impl Nip29JoinedGroupsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Runtime handle for one NIP-29 group-roster read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip29GroupRosterHandle(pub(super) ReadHandle);

impl Nip29GroupRosterHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}
