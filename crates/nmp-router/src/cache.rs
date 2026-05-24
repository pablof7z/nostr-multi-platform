//! `InMemoryMailboxCache` — the NIP-65 (kind:10002) cache. Single writer is
//! [`crate::Kind10002Parser`]; readers are [`crate::GenericOutboxRouter`] and
//! (post-step-3) the planner.

use std::collections::HashMap;
use std::sync::RwLock;

use nmp_core::substrate::{MailboxCache, ParsedRelayList, RoutingPubkey, RoutingRelayUrl};

#[derive(Default)]
pub struct InMemoryMailboxCache {
    inner: RwLock<HashMap<RoutingPubkey, ParsedRelayList>>,
}

impl InMemoryMailboxCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Diagnostic: number of authors currently cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().expect("RwLock poisoned").len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl MailboxCache for InMemoryMailboxCache {
    fn read_relays(&self, author: &RoutingPubkey) -> Option<Vec<RoutingRelayUrl>> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .get(author)
            .map(ParsedRelayList::read_set)
    }

    fn write_relays(&self, author: &RoutingPubkey) -> Option<Vec<RoutingRelayUrl>> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .get(author)
            .map(ParsedRelayList::write_set)
    }

    fn snapshot(&self, author: &RoutingPubkey) -> Option<ParsedRelayList> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .get(author)
            .cloned()
    }

    fn snapshot_all(&self) -> Vec<(RoutingPubkey, ParsedRelayList)> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn remove(&self, author: &RoutingPubkey) {
        self.inner.write().expect("RwLock poisoned").remove(author);
    }

    fn upsert(&self, author: RoutingPubkey, list: ParsedRelayList) {
        self.inner.write().expect("RwLock poisoned").insert(author, list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_then_read_round_trips() {
        let cache = InMemoryMailboxCache::new();
        let alice: RoutingPubkey = "alice".into();

        assert_eq!(cache.read_relays(&alice), None);
        cache.upsert(
            alice.clone(),
            ParsedRelayList {
                read: vec!["wss://r.example".into()],
                write: vec!["wss://w.example".into()],
                both: vec!["wss://b.example".into()],
            },
        );

        let read = cache.read_relays(&alice).unwrap();
        assert!(read.contains(&"wss://r.example".to_string()));
        assert!(read.contains(&"wss://b.example".to_string()));

        let write = cache.write_relays(&alice).unwrap();
        assert!(write.contains(&"wss://w.example".to_string()));
        assert!(write.contains(&"wss://b.example".to_string()));
    }

    #[test]
    fn upsert_replaces_previous_entry() {
        let cache = InMemoryMailboxCache::new();
        let alice: RoutingPubkey = "alice".into();

        cache.upsert(alice.clone(), ParsedRelayList {
            write: vec!["wss://old.example".into()],
            ..ParsedRelayList::default()
        });
        cache.upsert(alice.clone(), ParsedRelayList {
            write: vec!["wss://new.example".into()],
            ..ParsedRelayList::default()
        });

        assert_eq!(
            cache.write_relays(&alice),
            Some(vec!["wss://new.example".into()]),
        );
    }

    #[test]
    fn known_uses_default_impl() {
        let cache = InMemoryMailboxCache::new();
        let alice: RoutingPubkey = "alice".into();

        assert!(!cache.known(&alice));
        cache.upsert(alice.clone(), ParsedRelayList {
            read: vec!["wss://r.example".into()],
            ..ParsedRelayList::default()
        });
        assert!(cache.known(&alice));
    }

    #[test]
    fn len_tracks_unique_authors() {
        let cache = InMemoryMailboxCache::new();
        assert_eq!(cache.len(), 0);
        cache.upsert("alice".into(), ParsedRelayList::default());
        cache.upsert("bob".into(), ParsedRelayList::default());
        cache.upsert("alice".into(), ParsedRelayList::default()); // replace
        assert_eq!(cache.len(), 2);
    }
}
