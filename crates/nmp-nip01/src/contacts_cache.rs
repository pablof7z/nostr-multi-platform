//! `ContactsCache` — the NIP-02 kind:3 contact-list (follow-set) cache.
//!
//! # Overview
//!
//! NIP-02 kind:3 is the replaceable contact-list event: its `["p", <pubkey>]`
//! tags name the author's follow set. The kernel projects this into the active
//! account's follow-feed M2 interests, the `timeline_authors` relevance set, and
//! the RAM byte/eviction budget. Before ADR-0057 PR 3 the cache + parse lived in
//! the kernel (`seed_contacts: HashMap<String, Vec<String>>` + the
//! `ingest_contacts` arm). PR 3 moves them here, mirroring the kind:0
//! `ProfileCache` (PR 2) and the NIP-17 `DmRelayCache` migrations:
//!
//! * The **writer** is [`crate::Kind3Parser`] — an
//!   [`nmp_core::substrate::IngestParser`] registered with the kernel's
//!   [`nmp_core::substrate::EventIngestDispatcher`] at composition time.
//! * The **reader** is the kernel
//!   ([`nmp_core::substrate::ContactsLookup`] impl) — consulted by the
//!   follow-feed registration, the byte estimate, RAM eviction, and the
//!   `contacts_authors` diagnostic.
//!
//! The same `Arc<ContactsCache>` is wired on both ends at composition time; the
//! kernel sees it only as `Arc<dyn ContactsLookup>`. Crucially, the kernel does
//! NOT inline the planner/lifecycle side effects (follow-feed re-registration,
//! `timeline_authors` rebuild, cache-serve) into this parser — those are
//! kernel-owned and driven by the kernel's own contacts-transition detection in
//! `project_accepted_event`. The parser stays side-effect-free against kernel
//! state (the `IngestParser` contract).
//!
//! # Supersession (newest kind:3 wins)
//!
//! kind:3 is replaceable: the cache keeps the newest event per author. The
//! supersession rule mirrors the store's D4 logic exactly — strict `>` on
//! `created_at`, lexicographic event-id tiebreak on equal timestamps. The same
//! rule holds for relay-ingested, locally-published (read-your-writes), and
//! cache-served kind:3, all flowing through the unified ingest chokepoint
//! (`verify_and_persist` → dispatcher fan-out / `feed_served_event`) to this
//! cache.
//!
//! # `Some(vec![])` is NOT `None`
//!
//! A kind:3 with no `p` tags is a CLEARED follow set — stored as `Some(vec![])`,
//! a distinct state from "no kind:3 cached" (`None`). The kernel's
//! follow-feed registration relies on the cleared set to WITHDRAW the prior
//! follow-feed interests. See `nmp_core::substrate::ContactsLookup`.
//!
//! # D doctrine
//!
//! * **D0** — no kernel noun. The kernel handles only the `ContactsLookup`
//!   trait shape; the kind:3 wire format is confined to this crate.
//! * **D4** — single writer per fact. `Kind3Parser` is the only production
//!   writer; tests use [`Self::upsert_view`] directly. Interior mutability is
//!   an `RwLock<HashMap<…>>` so the parser's `&self` method can write.
//! * **D6** — a poisoned lock is a no-op / empty-case degrade rather than a
//!   panic. The cache methods log to `tracing` and degrade.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use nmp_core::substrate::{ContactsLookup, ContactsView};

/// In-memory NIP-02 kind:3 contact-list cache.
///
/// One entry per author pubkey, valued by the parsed [`ContactsView`] (the
/// full follow set + the source event's `created_at` / `event_id`). Cheap to
/// clone behind an `Arc`; grows only as kind:3 events arrive.
///
/// Wrapped in `Arc` at composition time so the same handle is the writer
/// (consumed by `Kind3Parser`) and the reader (consumed by the kernel as
/// `Arc<dyn ContactsLookup>`).
#[derive(Default)]
pub struct ContactsCache {
    inner: RwLock<HashMap<String, ContactsView>>,
}

impl ContactsCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert `pubkey`'s parsed contact list, applying the kind:3 supersession
    /// rule (newest `created_at` wins; lexicographically-smaller event-id wins
    /// on a tie). Returns `true` iff the candidate replaced the cached entry —
    /// the signal the kernel's contacts-transition detection uses to drive the
    /// active account's follow-feed effects.
    ///
    /// D4 — the single production writer is [`crate::Kind3Parser`]; tests may
    /// write directly. D6 — a poisoned lock is logged and dropped (no panic,
    /// no partial write); the dropped upsert reports `false` (no change).
    pub fn upsert_view(&self, pubkey: String, candidate: ContactsView) -> bool {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "ContactsCache write lock poisoned — dropping upsert (D6)"
                );
                return false;
            }
        };
        let should_replace = guard.get(&pubkey).is_none_or(|current| {
            candidate.created_at > current.created_at
                || (candidate.created_at == current.created_at
                    && candidate.event_id < current.event_id)
        });
        if should_replace {
            guard.insert(pubkey, candidate);
        }
        should_replace
    }
}

impl ContactsLookup for ContactsCache {
    fn follows(&self, pubkey: &str) -> Option<Vec<String>> {
        match self.inner.read() {
            Ok(guard) => guard.get(pubkey).map(|v| v.follows.clone()),
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "ContactsCache read lock poisoned — degrading to None (D6)"
                );
                None
            }
        }
    }

    fn upsert(&self, pubkey: String, view: ContactsView) -> bool {
        // Delegate to the inherent writer (single supersession code path); the
        // non-ingest sign-in seed (`Kernel::prepopulate_contacts`) reaches
        // this through the `ContactsLookup` trait object.
        self.upsert_view(pubkey, view)
    }

    fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    fn total_follows(&self) -> usize {
        self.inner
            .read()
            .map(|g| g.values().map(|v| v.follows.len()).sum())
            .unwrap_or(0)
    }

    fn evict_to(&self, pinned: &HashSet<String>, hwm: usize) -> usize {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        let len = guard.len();
        if len <= hwm {
            return 0;
        }
        // Oldest-first eviction: lowest `created_at`, lexicographic pubkey
        // tiebreak. Mirrors the kind:0 `ProfileCache::evict_to` ordering.
        let mut candidates: Vec<(String, u64)> = guard
            .iter()
            .filter_map(|(k, v)| {
                if pinned.contains(k) {
                    None
                } else {
                    Some((k.clone(), v.created_at))
                }
            })
            .collect();
        candidates.sort_unstable_by(|(ka, a), (kb, b)| a.cmp(b).then_with(|| ka.cmp(kb)));

        let to_remove = len - hwm;
        let mut removed = 0usize;
        for (key, _) in candidates.into_iter().take(to_remove) {
            if guard.remove(&key).is_some() {
                removed += 1;
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn view(event_id: &str, created_at: u64, follows: &[&str]) -> ContactsView {
        ContactsView {
            event_id: event_id.into(),
            created_at,
            follows: follows.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn cold_cache_is_empty() {
        let cache = ContactsCache::new();
        assert!(cache.follows("alice").is_none());
        assert!(cache.is_empty());
        assert_eq!(cache.total_follows(), 0);
    }

    #[test]
    fn upsert_then_read_round_trips() {
        let cache = ContactsCache::new();
        assert!(cache.upsert_view("alice".into(), view("aa", 100, &["x", "y"])));
        assert_eq!(
            cache.follows("alice"),
            Some(vec!["x".to_string(), "y".to_string()])
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_follows(), 2);
    }

    #[test]
    fn cleared_follow_set_is_some_empty() {
        let cache = ContactsCache::new();
        assert!(cache.upsert_view("alice".into(), view("aa", 100, &[])));
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
        assert_eq!(cache.follows("bob"), None);
    }

    #[test]
    fn newer_created_at_supersedes() {
        let cache = ContactsCache::new();
        assert!(cache.upsert_view("alice".into(), view("aa", 100, &["x"])));
        assert!(cache.upsert_view("alice".into(), view("bb", 200, &[])));
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
    }

    #[test]
    fn older_created_at_does_not_supersede() {
        let cache = ContactsCache::new();
        assert!(cache.upsert_view("alice".into(), view("bb", 200, &["x"])));
        assert!(!cache.upsert_view("alice".into(), view("aa", 100, &[])));
        assert_eq!(cache.follows("alice"), Some(vec!["x".to_string()]));
    }

    #[test]
    fn equal_created_at_lower_event_id_wins() {
        let cache = ContactsCache::new();
        assert!(cache.upsert_view("alice".into(), view("bb", 100, &["x"])));
        assert!(cache.upsert_view("alice".into(), view("aa", 100, &["y"])));
        assert_eq!(cache.follows("alice"), Some(vec!["y".to_string()]));
        assert!(!cache.upsert_view("alice".into(), view("cc", 100, &["z"])));
        assert_eq!(cache.follows("alice"), Some(vec!["y".to_string()]));
    }

    #[test]
    fn evict_to_drops_oldest_unpinned() {
        let cache = ContactsCache::new();
        cache.upsert_view("a".into(), view("a", 100, &[]));
        cache.upsert_view("b".into(), view("b", 200, &[]));
        cache.upsert_view("c".into(), view("c", 300, &[]));

        let mut pinned = HashSet::new();
        pinned.insert("a".to_string()); // pinned despite being oldest

        let removed = cache.evict_to(&pinned, 2);
        assert_eq!(removed, 1);
        assert!(cache.follows("a").is_some(), "pinned survives");
        assert!(cache.follows("b").is_none(), "oldest unpinned reaped");
        assert!(cache.follows("c").is_some());
    }

    #[test]
    fn satisfies_contacts_lookup_via_arc_dyn() {
        let cache = Arc::new(ContactsCache::new());
        cache.upsert_view("alice".into(), view("aa", 100, &["x"]));
        let as_trait: Arc<dyn ContactsLookup> = Arc::clone(&cache) as _;
        assert_eq!(as_trait.follows("alice").map(|f| f.len()), Some(1));
        assert_eq!(as_trait.len(), 1);
        assert_eq!(as_trait.total_follows(), 1);
    }
}
