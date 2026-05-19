//! `JoinedHostsCache` — `(pubkey, host_relay_url, local_id)` registry per
//! `routing.md` §4.3. Drives `JoinedGroupsView` fanout (one host-pinned
//! interest per host the user touches groups on).

use std::collections::{BTreeMap, BTreeSet};

use crate::group_id::{GroupId, RelayUrl};

#[derive(Clone, Debug, Default)]
pub struct JoinedHostsCache {
    /// pubkey -> host_relay_url -> set of local_ids
    by_pubkey: BTreeMap<String, BTreeMap<RelayUrl, BTreeSet<String>>>,
}

impl JoinedHostsCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record verified membership (from any of the four trusted sources in
    /// `routing.md` §4.3: own write, invite redeem, explicit import, verified
    /// bootstrap).
    pub fn insert(&mut self, pubkey: &str, group: &GroupId) {
        self.by_pubkey
            .entry(pubkey.to_string())
            .or_default()
            .entry(group.host_relay_url.clone())
            .or_default()
            .insert(group.local_id.clone());
    }

    /// All host relays carrying at least one group for `pubkey`. Used by
    /// `JoinedGroups::dependencies()` to fan out one `LogicalInterest` per
    /// host (`routing.md` §4.3 / §3.2 "Strategy C").
    pub fn hosts_for(&self, pubkey: &str) -> Vec<RelayUrl> {
        self.by_pubkey
            .get(pubkey)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn groups_for(&self, pubkey: &str, host: &str) -> Vec<String> {
        self.by_pubkey
            .get(pubkey)
            .and_then(|m| m.get(host))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_hosts_fans_out() {
        let mut jhc = JoinedHostsCache::new();
        jhc.insert("alice", &GroupId::new("wss://a", "g1"));
        jhc.insert("alice", &GroupId::new("wss://b", "g2"));
        jhc.insert("alice", &GroupId::new("wss://a", "g3"));
        let hosts = jhc.hosts_for("alice");
        assert_eq!(hosts.len(), 2);
        assert_eq!(jhc.groups_for("alice", "wss://a").len(), 2);
    }
}
