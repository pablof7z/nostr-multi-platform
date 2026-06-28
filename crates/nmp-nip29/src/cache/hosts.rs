//! `JoinedHostsCache` — `(pubkey, host_relay_url, local_id)` registry per
//! `routing.md` §4.3. Drives `JoinedGroupsView` fanout (one host-pinned
//! interest per host the user touches groups on).
//!
//! ## Persistence (D4-compliant)
//!
//! [`JoinedHostsCache::open`] loads existing membership rows from the
//! `nmp.nip29.joined_hosts` domain namespace on startup. Every call to
//! [`insert`](JoinedHostsCache::insert) writes the new row through to the
//! store immediately. Single-writer per D4.

use std::collections::{BTreeMap, BTreeSet};

use nmp_store::{DomainHandle, EventStore, StoreError};

use crate::group_id::{GroupId, RelayUrl};

/// Domain namespace for the durable NIP-29 joined-hosts cache.
const JOINED_NAMESPACE: &'static str = "nmp.nip29.joined_hosts";

pub struct JoinedHostsCache {
    /// pubkey -> `host_relay_url` -> set of `local_ids`
    by_pubkey: BTreeMap<String, BTreeMap<RelayUrl, BTreeSet<String>>>,
    /// Durable domain handle. `None` in the pure in-memory variant (tests).
    /// `Some` in the persistent variant opened via [`Self::open`].
    domain: Option<DomainHandle>,
}

impl Default for JoinedHostsCache {
    fn default() -> Self {
        Self {
            by_pubkey: BTreeMap::new(),
            domain: None,
        }
    }
}

impl JoinedHostsCache {
    /// Construct a pure in-memory cache (no persistence). For tests and
    /// contexts that have no durable store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a store-backed, durable cache.
    ///
    /// Loads existing membership rows from the `nmp.nip29.joined_hosts` domain
    /// namespace; subsequent [`insert`](Self::insert) calls write through to
    /// the store immediately.
    ///
    /// Single-writer per D4: the caller serialises access via whatever
    /// synchronisation primitive owns the returned struct.
    ///
    /// # Errors
    ///
    /// Returns `StoreError` if the domain namespace cannot be opened or if the
    /// startup scan fails.
    pub fn open(store: &dyn EventStore) -> Result<Self, StoreError> {
        let domain = store.domain_open(JOINED_NAMESPACE)?;
        let mut cache = Self {
            domain: Some(domain),
            ..Default::default()
        };
        cache.load()?;
        Ok(cache)
    }

    /// Record verified membership (from any of the four trusted sources in
    /// `routing.md` §4.3: own write, invite redeem, explicit import, verified
    /// bootstrap).
    pub fn insert(&mut self, pubkey: &str, group: &GroupId) {
        let is_new = self
            .by_pubkey
            .entry(pubkey.to_string())
            .or_default()
            .entry(group.host_relay_url.clone())
            .or_default()
            .insert(group.local_id.clone());
        if is_new {
            self.persist_membership(pubkey, group);
        }
    }

    /// All host relays carrying at least one group for `pubkey`. Used by
    /// `JoinedGroups::dependencies()` to fan out one `LogicalInterest` per
    /// host (`routing.md` §4.3 / §3.2 "Strategy C").
    #[must_use]
    pub fn hosts_for(&self, pubkey: &str) -> Vec<RelayUrl> {
        self.by_pubkey
            .get(pubkey)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn groups_for(&self, pubkey: &str, host: &str) -> Vec<String> {
        self.by_pubkey
            .get(pubkey)
            .and_then(|m| m.get(host))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    // ── Persistence helpers ───────────────────────────────────────────────────

    /// Load membership rows from the domain store into memory on startup.
    fn load(&mut self) -> Result<(), StoreError> {
        let rows = match &self.domain {
            Some(d) => d.scan_prefix(b"")?.collect::<Result<Vec<_>, _>>()?,
            None => return Ok(()),
        };
        for (key, _val) in rows {
            // key layout: pubkey_bytes, 0x00, host_relay_url_bytes, 0x00, local_id_bytes
            if let Some((pubkey, rest)) = split_once_null(&key) {
                if let Some((host, local)) = split_once_null(rest) {
                    if let (Ok(pk), Ok(h), Ok(l)) = (
                        std::str::from_utf8(pubkey),
                        std::str::from_utf8(host),
                        std::str::from_utf8(local),
                    ) {
                        if !pk.is_empty() && !h.is_empty() && !l.is_empty() {
                            self.by_pubkey
                                .entry(pk.to_string())
                                .or_default()
                                .entry(h.to_string())
                                .or_default()
                                .insert(l.to_string());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Write-through: persist a membership row to the domain store.
    fn persist_membership(&self, pubkey: &str, group: &GroupId) {
        if let Some(d) = &self.domain {
            let key = membership_key(pubkey, group);
            let _ = d.put(&key, b"1");
        }
    }
}

/// Split `bytes` on the first null byte, returning `(before, after)` where
/// `after` does NOT include the null byte. Returns `None` if no null found.
fn split_once_null(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let pos = bytes.iter().position(|&b| b == 0)?;
    Some((&bytes[..pos], &bytes[pos + 1..]))
}

/// Build the domain key for a membership row.
///
/// Format: `pubkey_bytes, 0x00, host_relay_url_bytes, 0x00, local_id_bytes`
/// All three components are ASCII-safe with no embedded null bytes (pubkeys
/// are hex, relay URLs are URLs, local IDs are `[a-z0-9-_]+`).
fn membership_key(pubkey: &str, group: &GroupId) -> Vec<u8> {
    let mut k = Vec::with_capacity(
        pubkey.len() + 1 + group.host_relay_url.len() + 1 + group.local_id.len(),
    );
    k.extend_from_slice(pubkey.as_bytes());
    k.push(0u8);
    k.extend_from_slice(group.host_relay_url.as_bytes());
    k.push(0u8);
    k.extend_from_slice(group.local_id.as_bytes());
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_store::MemEventStore;

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

    // ── Persistence tests ─────────────────────────────────────────────────────

    /// Membership rows survive a simulated restart (re-open the same
    /// MemEventStore, reconstruct the cache, verify hosts/groups are intact).
    #[test]
    fn joined_hosts_membership_survives_restart() {
        let store = MemEventStore::new();
        let g1 = GroupId::new("wss://relay.example.com", "room-a");
        let g2 = GroupId::new("wss://relay.example.com", "room-b");
        let g3 = GroupId::new("wss://other.example.com", "room-c");

        // Session 1: register membership.
        {
            let mut cache = JoinedHostsCache::open(&store).expect("open session 1");
            cache.insert("alice-pk", &g1);
            cache.insert("alice-pk", &g2);
            cache.insert("alice-pk", &g3);
        }

        // Session 2: re-open the same store — membership must be loaded.
        {
            let cache = JoinedHostsCache::open(&store).expect("open session 2");
            let hosts = cache.hosts_for("alice-pk");
            assert_eq!(hosts.len(), 2, "two distinct hosts must survive restart");
            assert!(
                hosts.contains(&"wss://relay.example.com".to_string()),
                "first host present"
            );
            assert!(
                hosts.contains(&"wss://other.example.com".to_string()),
                "second host present"
            );
            let relay_groups = cache.groups_for("alice-pk", "wss://relay.example.com");
            assert_eq!(relay_groups.len(), 2, "two groups on first host must survive");
            assert!(relay_groups.contains(&"room-a".to_string()));
            assert!(relay_groups.contains(&"room-b".to_string()));
            let other_groups = cache.groups_for("alice-pk", "wss://other.example.com");
            assert_eq!(other_groups.len(), 1);
            assert!(other_groups.contains(&"room-c".to_string()));
        }
    }

    /// Multiple pubkeys each persist their own membership independently.
    #[test]
    fn joined_hosts_multi_pubkey_persists_independently() {
        let store = MemEventStore::new();
        let g = GroupId::new("wss://relay.example.com", "shared-room");

        {
            let mut cache = JoinedHostsCache::open(&store).expect("open");
            cache.insert("alice-pk", &g);
            cache.insert("bob-pk", &g);
        }

        {
            let cache = JoinedHostsCache::open(&store).expect("reopen");
            assert_eq!(cache.hosts_for("alice-pk").len(), 1, "alice must have one host");
            assert_eq!(cache.hosts_for("bob-pk").len(), 1, "bob must have one host");
            assert_eq!(
                cache.groups_for("alice-pk", "wss://relay.example.com").len(),
                1
            );
            assert_eq!(
                cache.groups_for("bob-pk", "wss://relay.example.com").len(),
                1
            );
        }
    }

    /// Duplicate inserts do not write duplicate rows (idempotent insert).
    #[test]
    fn joined_hosts_duplicate_insert_is_idempotent() {
        let store = MemEventStore::new();
        let g = GroupId::new("wss://relay.example.com", "room");

        {
            let mut cache = JoinedHostsCache::open(&store).expect("open");
            cache.insert("alice-pk", &g);
            cache.insert("alice-pk", &g); // duplicate
        }

        {
            let cache = JoinedHostsCache::open(&store).expect("reopen");
            // Should see exactly one group, not two
            assert_eq!(
                cache.groups_for("alice-pk", "wss://relay.example.com").len(),
                1
            );
        }
    }
}
