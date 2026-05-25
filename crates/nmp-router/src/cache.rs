//! `InMemoryMailboxCache` — the NIP-65 (kind:10002) cache. Single writer is
//! [`crate::Kind10002Parser`]; readers are [`crate::GenericOutboxRouter`] and
//! (post-step-3) the planner.
//!
//! # Lock-poisoning policy (D15)
//!
//! Every `self.inner.read()` / `self.inner.write()` returns a `Result` whose
//! `Err` variant signals lock poisoning — another thread panicked while
//! holding the lock. The fast-path router and the planner read this cache on
//! every routing decision, so a panicking writer must NOT cascade into a
//! propagated panic on every subsequent reader: that would convert one local
//! failure into an actor-wide kill switch.
//!
//! Policy: degrade gracefully on poison.
//!
//! - Read paths (`read_relays`, `write_relays`, `snapshot`, `snapshot_all`,
//!   `len`, `is_empty`) treat a poisoned lock as "no data" — return `None` /
//!   empty `Vec` / `0`. The router's lane 1 then sees the cache as cold and
//!   either falls back to lane 7 (AppRelay) or returns `Unroutable`, which
//!   the kernel observes as a routing failure rather than a process crash.
//! - Write paths (`upsert`, `remove`) silently drop the mutation on poison.
//!   The next successful writer will overwrite; in the worst case the cache
//!   stays stale until the next kind:10002 arrives and the parser retries.
//!
//! D15 (host-closure / substrate-panic discipline): every panicking surface
//! the kernel's actor thread crosses must fail gracefully, not amplify into
//! a process kill. The cache is the canonical example — a router lookup on
//! the actor thread must never unwind because a background thread (e.g. a
//! test harness, an FFI worker) poisoned the lock.

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

    /// Diagnostic: number of authors currently cached. Returns `0` on a
    /// poisoned lock (degrade-gracefully policy — see module docs).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
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
            .ok()
            .and_then(|g| g.get(author).map(ParsedRelayList::read_set))
    }

    fn write_relays(&self, author: &RoutingPubkey) -> Option<Vec<RoutingRelayUrl>> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(author).map(ParsedRelayList::write_set))
    }

    fn snapshot(&self, author: &RoutingPubkey) -> Option<ParsedRelayList> {
        self.inner.read().ok().and_then(|g| g.get(author).cloned())
    }

    fn snapshot_all(&self) -> Vec<(RoutingPubkey, ParsedRelayList)> {
        self.inner
            .read()
            .map(|g| g.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    fn remove(&self, author: &RoutingPubkey) {
        if let Ok(mut g) = self.inner.write() {
            g.remove(author);
        }
    }

    fn upsert(&self, author: RoutingPubkey, list: ParsedRelayList) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(author, list);
        }
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

    // D15 — verify that a poisoned lock degrades gracefully rather than
    // amplifying into a propagated panic on every subsequent reader.
    //
    // We poison the inner `RwLock` by panicking a thread that holds the
    // write guard, then drive every read/write API and assert each returns
    // the "no data" / silent-no-op fallback documented in the module
    // policy.
    #[test]
    fn poisoned_lock_degrades_gracefully() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(
            "alice".into(),
            ParsedRelayList {
                write: vec!["wss://w.example".into()],
                ..ParsedRelayList::default()
            },
        );

        // Poison the lock: a thread that panics while holding the write
        // guard leaves the RwLock in a poisoned state. The thread join
        // returns the propagated panic payload — we discard it.
        let poisoner = Arc::clone(&cache);
        let _ = thread::spawn(move || {
            let _guard = poisoner.inner.write().expect("acquire write to poison");
            panic!("intentional — poison the inner lock");
        })
        .join();

        // Every read API returns the empty / cold fallback, not a panic.
        assert_eq!(cache.read_relays(&"alice".to_string()), None);
        assert_eq!(cache.write_relays(&"alice".to_string()), None);
        assert!(cache.snapshot(&"alice".to_string()).is_none());
        assert!(cache.snapshot_all().is_empty());
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        // Write APIs silently no-op (no panic, mutation lost — by policy).
        cache.upsert(
            "bob".into(),
            ParsedRelayList {
                write: vec!["wss://b.example".into()],
                ..ParsedRelayList::default()
            },
        );
        cache.remove(&"alice".to_string());
        // Post-write reads still degrade — the lock stays poisoned.
        assert_eq!(cache.write_relays(&"bob".to_string()), None);
    }
}
