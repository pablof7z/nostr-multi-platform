//! `ContactsLookup` — substrate-generic seam for a per-pubkey kind:3
//! contact-list (follow-set) cache.
//!
//! The kernel needs to ask "for author `P`, what is P's cached follow set?"
//! when it (re)registers the active account's follow-feed M2 interests, rebuilds
//! the `timeline_authors` projection, and accounts for the contacts cache in the
//! RAM-tier byte/eviction budget. Before ADR-0057 PR 3 this lived in a
//! kernel-owned `seed_contacts: HashMap<String, Vec<String>>` field with a
//! hardwired kind:3 ingest arm (`ingest_contacts`) — both NIP-02 nouns inside
//! `nmp-core`. PR 3 moves the CACHE + PARSE out behind this trait, mirroring the
//! NIP-01 [`crate::substrate::ProfileLookup`] / NIP-17
//! [`crate::substrate::DmInboxRelayLookup`] migrations:
//!
//! - The **writer** is `nmp-nip01`'s `Kind3Parser`
//!   ([`crate::substrate::IngestParser`]) — registered with the kernel's
//!   [`crate::substrate::EventIngestDispatcher`] at composition time. It owns
//!   the supersession rule (newest kind:3 wins, lexicographic event-id tiebreak)
//!   and extracts the follow set via `nmp_core::tags::contact_follows`
//!   (the SAME pure function the kernel's old `ingest_contacts` used, so the
//!   valid-hex-p-tags extraction is byte-identical).
//! - The **reader** is the kernel/defaults composition (`ActiveFollowSet`,
//!   ReducedSource feed-session compilation, the byte estimate, RAM eviction,
//!   and the diagnostic `contacts_authors` counter) — it consults this trait
//!   through a substrate-generic shape and never names the kind:3 wire format
//!   (D0).
//!
//! Both ends agree on a shared `Arc` (the concrete `nmp_nip01::ContactsCache`)
//! at composition time; the kernel sees it only as `Arc<dyn ContactsLookup>`.
//! A kernel built without any contacts backend uses [`EmptyContactsLookup`],
//! which reports an always-empty cache (every lookup returns `None`).
//!
//! ## Why `Some(vec![])` is NOT `None`
//!
//! Unlike [`crate::substrate::DmInboxRelayLookup`] (which collapses an empty
//! list to `None` so the gift-wrap publish path fails closed), the contacts
//! cache MUST distinguish "this author published a kind:3 with no `p` tags"
//! (a CLEARED follow set → `Some(vec![])`) from "no kind:3 has arrived yet"
//! (`None`). The kernel's old `ingest_contacts` stored an empty `Vec` for an
//! empty kind:3 (a cleared follow set is a real state, not the absence of
//! data), and the ReducedSource feed-session path relies on the empty vector to
//! WITHDRAW the prior active-follows dependent interests. The cache preserves
//! that distinction.

use std::sync::Arc;

/// Protocol-neutral view of a cached kind:3 contact list.
///
/// Carries the parsed follow set plus the source `(event_id, created_at)` the
/// cache needs for supersession (newest kind:3 wins) and RAM-eviction LRU
/// ordering. The follow set is the document-order list of valid-hex
/// `p`-tag pubkeys (see `nmp_core::tags::contact_follows`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactsView {
    /// Source kind:3 event id (used by the supersession tiebreak + diagnostics).
    pub event_id: String,
    /// Source kind:3 `created_at` (Unix seconds). Drives both supersession
    /// (newest wins) and the RAM-eviction LRU ordering.
    pub created_at: u64,
    /// The full follow set (valid-hex `p`-tag pubkeys, in document order;
    /// uncapped since #1497).
    /// An empty vector is a CLEARED follow set (a real, distinct state from
    /// "no kind:3 cached" — see the module docs).
    pub follows: Vec<String>,
}

/// Lookup + eviction contract over a per-pubkey kind:3 contact-list cache.
///
/// Implementations MUST use interior mutability for the backing store — every
/// method takes `&self`. The writer side (the kind:3 ingest parser) drives
/// supersession through a different method on the concrete type.
pub trait ContactsLookup: Send + Sync {
    /// Resolve `pubkey`'s cached follow set, or `None` when no kind:3 is cached.
    /// `Some(vec![])` (a cleared follow set) is distinct from `None`.
    /// `pubkey` is lowercase hex.
    fn follows(&self, pubkey: &str) -> Option<Vec<String>>;

    /// Direct cache writer — upsert `pubkey`'s follow set, applying the kind:3
    /// supersession rule (newest `created_at` wins; lexicographically-smaller
    /// event-id wins on a tie). Returns `true` iff the candidate replaced the
    /// cached entry.
    ///
    /// The PRODUCTION ingest writer is `nmp_nip01::Kind3Parser` (via the
    /// `EventIngestDispatcher`); this trait method is the **non-ingest** writer
    /// seam the kernel's sign-in seed (`Kernel::prepopulate_contacts`) uses
    /// to restore KNOWN contacts directly into the cache — WITHOUT fabricating a
    /// kind:3 event through the observer fan-out. It mirrors
    /// [`crate::substrate::MailboxCache`]'s writer, the analogous capability
    /// whose sign-in seed (`Kernel::prepopulate_author_relay_list`) writes the
    /// cache directly too. The kernel never names the kind:3 wire format (D0).
    fn upsert(&self, pubkey: String, view: ContactsView) -> bool;

    /// Number of cached contact lists (distinct authors). Mirrors the kernel's
    /// former `seed_contacts.len()` — feeds the `stored_events` diagnostic and
    /// the RAM-eviction watermark check.
    fn len(&self) -> usize;

    /// `true` iff no contact list is cached.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total number of follow entries across all cached lists (sum of every
    /// list's length). Mirrors the kernel's former
    /// `seed_contacts.values().map(Vec::len).sum()` — feeds the
    /// `contacts_authors` diagnostic counter and the byte estimate.
    fn total_follows(&self) -> usize;

    /// Estimated heap bytes held by the cached contact lists, for the
    /// `estimated_store_bytes` diagnostic. Returns the same formula the kernel's
    /// former `compute_estimated_store_bytes` contacts term used
    /// (`total_follows() * 64`).
    fn estimated_bytes(&self) -> usize {
        self.total_follows() * 64
    }

    /// Pin-aware RAM eviction (#1088 RAM tier). Bring the cache down to `hwm`
    /// entries by removing entries whose pubkey is NOT in `pinned`, lowest
    /// `created_at` first with a lexicographic-pubkey tiebreak. Returns the
    /// number of entries removed.
    ///
    /// The kernel computes `pinned` (the active account) and passes it in — the
    /// cache owns the mechanism, the kernel owns the policy. A no-op when the
    /// cache already sits at/below `hwm`.
    fn evict_to(&self, pinned: &std::collections::HashSet<String>, hwm: usize) -> usize;
}

/// Default backing — the kernel cold-start cache. Always empty; every
/// `follows` lookup returns `None` (the "no kind:3 has arrived" branch the
/// follow-feed registration already expects — an empty follow set withdraws any
/// stale interests).
#[derive(Default)]
pub struct EmptyContactsLookup;

impl ContactsLookup for EmptyContactsLookup {
    fn follows(&self, _pubkey: &str) -> Option<Vec<String>> {
        None
    }
    fn upsert(&self, _pubkey: String, _view: ContactsView) -> bool {
        // Cold-start no-op backing: there is nothing to write into. Production
        // composition installs `nmp_nip01::ContactsCache` before any sign-in
        // seed runs (`set_contacts_lookup`).
        false
    }
    fn len(&self) -> usize {
        0
    }
    fn total_follows(&self) -> usize {
        0
    }
    fn evict_to(&self, _pinned: &std::collections::HashSet<String>, _hwm: usize) -> usize {
        0
    }
}

/// Convenience: a fresh `Arc<dyn ContactsLookup>` backed by
/// [`EmptyContactsLookup`] — the kernel's default before a contacts cache is
/// wired in.
#[must_use]
pub fn empty_contacts_lookup() -> Arc<dyn ContactsLookup> {
    Arc::new(EmptyContactsLookup)
}

/// Test-only in-memory cache for the substrate `ContactsLookup` trait.
///
/// Production composition uses `nmp_nip01::ContactsCache`. This stand-in lives
/// inside `nmp-core` so the crate's own tests (which cannot depend on
/// `nmp-nip01`) can still exercise the follow-feed registration / byte-estimate
/// / RAM-eviction readers AND the chokepoint→parser→effect-signal path
/// end-to-end. Mirrors the production cache's shape, supersession rule, eviction
/// ordering, and parse contract so a test that seeds through this helper
/// produces the same cache shape the production `Kind3Parser` would.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct TestContactsCache {
    inner: std::sync::RwLock<std::collections::HashMap<String, ContactsView>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestContactsCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert a pre-built [`ContactsView`] under `pubkey`, applying the kind:3
    /// supersession rule (newest `created_at` wins; lexicographically-smaller
    /// event-id wins on a tie). Returns `true` iff the candidate replaced the
    /// cached entry. Mirrors `nmp_nip01::ContactsCache::upsert_view`.
    pub fn upsert_view(&self, pubkey: &str, candidate: ContactsView) -> bool {
        let Ok(mut guard) = self.inner.write() else {
            return false;
        };
        let should_replace = guard.get(pubkey).is_none_or(|current| {
            candidate.created_at > current.created_at
                || (candidate.created_at == current.created_at
                    && candidate.event_id < current.event_id)
        });
        if should_replace {
            guard.insert(pubkey.to_string(), candidate);
        }
        should_replace
    }

    /// Clear all cached contact lists — the in-crate equivalent of a cold
    /// restart losing the in-memory cache (which production rebuilds from the
    /// store via cache-serve). Keeps the same backing `Arc` so a registered
    /// `TestKind3Parser` and the kernel's `contacts_lookup` reader stay in sync.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.clear();
        }
    }

    /// Parse a kind:3 event's `(event_id, created_at, tags)` into a
    /// [`ContactsView`] (extracting the follow set via
    /// `crate::tags::contact_follows`) and upsert it. The parse contract
    /// is a verbatim port of the production `nmp_nip01::Kind3Parser` so
    /// test-seeded contacts match production exactly. Returns whether the
    /// candidate superseded the cached entry.
    pub fn ingest_kind3(
        &self,
        pubkey: &str,
        event_id: &str,
        created_at: u64,
        tags: &[Vec<String>],
    ) -> bool {
        let follows = crate::tags::contact_follows(tags);
        self.upsert_view(
            pubkey,
            ContactsView {
                event_id: event_id.to_string(),
                created_at,
                follows,
            },
        )
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ContactsLookup for TestContactsCache {
    fn follows(&self, pubkey: &str) -> Option<Vec<String>> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(pubkey).map(|v| v.follows.clone()))
    }
    fn upsert(&self, pubkey: String, view: ContactsView) -> bool {
        self.upsert_view(&pubkey, view)
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
    fn evict_to(&self, pinned: &std::collections::HashSet<String>, hwm: usize) -> usize {
        let Ok(mut guard) = self.inner.write() else {
            return 0;
        };
        let len = guard.len();
        if len <= hwm {
            return 0;
        }
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

/// Test-only kind:3 [`IngestParser`](crate::substrate::IngestParser) that writes
/// a [`TestContactsCache`].
///
/// Production composition registers `nmp_nip01::Kind3Parser`; this stand-in
/// lets `nmp-core`'s own tests exercise the real chokepoint path
/// (`verify_and_persist` → `EventIngestDispatcher` → parser → the kernel's
/// contacts-transition effect signal) — so a local kind:3 publish gets
/// read-your-writes through the SAME dispatcher fan-out production uses, without
/// depending on `nmp-nip01`. Registered on the test kernel's dispatcher at
/// construction (mirroring how the production parser is registered by
/// `nmp_substrate::install`).
#[cfg(any(test, feature = "test-support"))]
pub struct TestKind3Parser {
    cache: std::sync::Arc<TestContactsCache>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestKind3Parser {
    #[must_use]
    pub fn new(cache: std::sync::Arc<TestContactsCache>) -> Self {
        Self { cache }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl crate::substrate::IngestParser for TestKind3Parser {
    fn parse(&self, evt: &crate::store::VerifiedEvent) {
        let raw = evt.raw();
        if raw.kind != 3 {
            return;
        }
        self.cache
            .ingest_kind3(&raw.pubkey, &raw.id, raw.created_at, &raw.tags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_lookup_is_empty() {
        let lookup: Arc<dyn ContactsLookup> = empty_contacts_lookup();
        assert!(lookup.follows("alice").is_none());
        assert!(lookup.is_empty());
        assert_eq!(lookup.len(), 0);
        assert_eq!(lookup.total_follows(), 0);
        assert_eq!(lookup.evict_to(&HashSet::new(), 0), 0);
    }

    #[test]
    fn cleared_follow_set_is_some_empty_not_none() {
        let cache = TestContactsCache::new();
        // A kind:3 with no `p` tags → a CLEARED follow set: present but empty.
        assert!(cache.ingest_kind3("alice", "aa", 100, &[]));
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
        // Distinct from a never-seen author.
        assert_eq!(cache.follows("bob"), None);
    }

    #[test]
    fn ingest_extracts_capped_hex_p_tags_in_order() {
        let cache = TestContactsCache::new();
        let a = "1".repeat(64);
        let b = "2".repeat(64);
        let tags = vec![
            vec!["p".to_string(), a.clone()],
            vec!["p".to_string(), b.clone()],
            vec!["p".to_string(), "not-hex".to_string()],
            vec!["e".to_string(), a.clone()],
        ];
        cache.ingest_kind3("alice", "aa", 100, &tags);
        assert_eq!(cache.follows("alice"), Some(vec![a, b]));
    }

    #[test]
    fn newer_kind3_supersedes() {
        let cache = TestContactsCache::new();
        let a = "1".repeat(64);
        cache.ingest_kind3("alice", "old", 100, &[vec!["p".to_string(), a.clone()]]);
        cache.ingest_kind3("alice", "new", 200, &[]);
        // Newer (cleared) list wins.
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
        // Older does not replace newer.
        cache.ingest_kind3("alice", "stale", 150, &[vec!["p".to_string(), a]]);
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
    }

    #[test]
    fn evict_to_drops_oldest_unpinned_keeps_pinned() {
        let cache = TestContactsCache::new();
        cache.ingest_kind3("a", "a", 100, &[]);
        cache.ingest_kind3("b", "b", 200, &[]);
        cache.ingest_kind3("c", "c", 300, &[]);

        let mut pinned = HashSet::new();
        pinned.insert("a".to_string()); // pinned despite being oldest

        // bring to hwm=2: must remove exactly 1 unpinned, oldest-first → "b"
        let removed = cache.evict_to(&pinned, 2);
        assert_eq!(removed, 1);
        assert!(cache.follows("a").is_some(), "pinned survives");
        assert!(cache.follows("b").is_none(), "oldest unpinned reaped");
        assert!(cache.follows("c").is_some());
    }

    #[test]
    fn satisfies_contacts_lookup_via_arc_dyn() {
        let cache = Arc::new(TestContactsCache::new());
        cache.ingest_kind3("alice", "aa", 100, &[vec!["p".to_string(), "1".repeat(64)]]);
        let as_trait: Arc<dyn ContactsLookup> = Arc::clone(&cache) as _;
        assert_eq!(as_trait.follows("alice").map(|f| f.len()), Some(1));
        assert_eq!(as_trait.len(), 1);
        assert_eq!(as_trait.total_follows(), 1);
    }
}
