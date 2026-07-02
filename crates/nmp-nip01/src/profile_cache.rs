//! `ProfileCache` — the NIP-01 kind:0 profile-metadata cache.
//!
//! # Overview
//!
//! NIP-01 kind:0 is the replaceable profile-metadata event: a JSON object
//! carrying `name` / `display_name` / `picture` / `nip05` / `about` and the
//! NIP-57 `lud16` / `lud06` lightning fields. The kernel projects this data
//! into timeline-item enrichment, profile cards, the zap LNURL gate, and the
//! claim/TTL dedup gates. Before ADR-0070 PR 2 the cache + parse lived in the
//! kernel (`profiles: HashMap<String, Profile>` + the `ingest_profile` arm) —
//! both NIP-01 nouns inside `nmp-core`. PR 2 moves them here, mirroring the
//! NIP-17 `DmRelayCache` migration:
//!
//! * The **writer** is [`crate::Kind0Parser`] — an
//!   [`nmp_core::substrate::IngestParser`] registered with the kernel's
//!   [`nmp_core::substrate::EventIngestDispatcher`] at composition time.
//! * The **reader** is the kernel
//!   ([`nmp_core::substrate::ProfileLookup`] impl) — consulted by the
//!   enrichment path, the profile-claim TTL gate, RAM eviction, and the
//!   zap LNURL resolver.
//!
//! The same `Arc<ProfileCache>` is wired on both ends at composition time; the
//! kernel sees it only as `Arc<dyn ProfileLookup>`.
//!
//! # Supersession (newest kind:0 wins)
//!
//! kind:0 is replaceable: the cache keeps the newest event per author. The
//! supersession rule mirrors the store's D4 logic exactly — strict `>` on
//! `created_at`, lexicographic event-id tiebreak on equal timestamps (the
//! `< current.event_id` clause the kernel's old `ingest_profile` used). The
//! same rule holds for both relay-ingested and locally-published kind:0
//! (read-your-writes): both flow through the unified ingest chokepoint
//! (`verify_and_persist` → dispatcher fan-out) and reach this cache.
//!
//! # D doctrine
//!
//! * **D0** — no kernel noun. The kernel handles only the `ProfileLookup`
//!   trait shape; the kind:0 wire format is confined to this crate.
//! * **D4** — single writer per fact. `Kind0Parser` is the only production
//!   writer; tests use [`Self::upsert_view`] directly. Interior mutability is
//!   an `RwLock<HashMap<…>>` so the parser's `&self` method can write.
//! * **D6** — a poisoned lock is a no-op / empty-case degrade rather than a
//!   panic. The cache methods log to `tracing` and degrade.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use nmp_core::substrate::{ProfileLookup, ProfileView};

/// In-memory NIP-01 kind:0 profile cache.
///
/// One entry per author pubkey, valued by the parsed [`ProfileView`]. Cheap to
/// clone behind an `Arc` — every field is `Default`-constructed empty and grows
/// only as kind:0 events arrive.
///
/// Wrapped in `Arc` at composition time so the same handle is the writer
/// (consumed by `Kind0Parser`) and the reader (consumed by the kernel as
/// `Arc<dyn ProfileLookup>`).
#[derive(Default)]
pub struct ProfileCache {
    inner: RwLock<HashMap<String, ProfileView>>,
}

impl ProfileCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert `pubkey`'s parsed profile, applying the kind:0 supersession rule
    /// (newest `created_at` wins; lexicographically-smaller event-id wins on a
    /// tie). Returns `true` iff the candidate replaced the cached entry — the
    /// signal the kernel's wildcard ingest arm uses to bump `profiles_ver` and
    /// re-emit profile-derived projections.
    ///
    /// D4 — the single production writer is [`crate::Kind0Parser`]; tests may
    /// write directly. D6 — a poisoned lock is logged and dropped (no panic,
    /// no partial write); the dropped upsert reports `false` (no change).
    pub fn upsert_view(&self, pubkey: String, candidate: ProfileView) -> bool {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "ProfileCache write lock poisoned — dropping upsert (D6)"
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

impl ProfileLookup for ProfileCache {
    fn profile(&self, pubkey: &str) -> Option<ProfileView> {
        match self.inner.read() {
            Ok(guard) => guard.get(pubkey).cloned(),
            Err(e) => {
                tracing::warn!(
                    pubkey = %pubkey,
                    error = ?e,
                    "ProfileCache read lock poisoned — degrading to None (D6)"
                );
                None
            }
        }
    }

    fn contains(&self, pubkey: &str) -> bool {
        self.inner
            .read()
            .map(|g| g.contains_key(pubkey))
            .unwrap_or(false)
    }

    fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    fn estimated_bytes(&self) -> usize {
        // Same per-entry formula the kernel's former
        // `compute_estimated_store_bytes` profile term used (event_id + display
        // + picture_url + nip05 + about + a 96-byte struct/overhead constant).
        self.inner
            .read()
            .map(|g| {
                g.values()
                    .map(|p| {
                        p.event_id.len()
                            + p.display.len()
                            + p.picture_url.as_ref().map_or(0, String::len)
                            + p.nip05.len()
                            + p.about.len()
                            + 96
                    })
                    .sum()
            })
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
        // tiebreak. Mirrors the kernel's former `evict_profiles_cache`
        // ordering exactly so the RAM-tier behaviour is byte-identical.
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

    fn view(event_id: &str, created_at: u64) -> ProfileView {
        ProfileView {
            event_id: event_id.into(),
            created_at,
            display: "n".into(),
            ..Default::default()
        }
    }

    #[test]
    fn cold_cache_is_empty() {
        let cache = ProfileCache::new();
        assert!(cache.profile("alice").is_none());
        assert!(!cache.contains("alice"));
        assert!(cache.is_empty());
    }

    #[test]
    fn upsert_then_read_round_trips() {
        let cache = ProfileCache::new();
        assert!(cache.upsert_view("alice".into(), view("aa", 100)));
        let got = cache.profile("alice").expect("populated");
        assert_eq!(got.event_id, "aa");
        assert_eq!(got.created_at, 100);
        assert!(cache.contains("alice"));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn newer_created_at_supersedes() {
        let cache = ProfileCache::new();
        assert!(cache.upsert_view("alice".into(), view("aa", 100)));
        assert!(cache.upsert_view("alice".into(), view("bb", 200)));
        assert_eq!(cache.profile("alice").expect("cached").event_id, "bb");
    }

    #[test]
    fn older_created_at_does_not_supersede() {
        let cache = ProfileCache::new();
        assert!(cache.upsert_view("alice".into(), view("bb", 200)));
        assert!(!cache.upsert_view("alice".into(), view("aa", 100)));
        assert_eq!(cache.profile("alice").expect("cached").event_id, "bb");
    }

    #[test]
    fn equal_created_at_lower_event_id_wins() {
        let cache = ProfileCache::new();
        assert!(cache.upsert_view("alice".into(), view("bb", 100)));
        // lexicographically smaller id supersedes on a timestamp tie
        assert!(cache.upsert_view("alice".into(), view("aa", 100)));
        assert_eq!(cache.profile("alice").expect("cached").event_id, "aa");
        // a larger id on the same tie does NOT replace
        assert!(!cache.upsert_view("alice".into(), view("cc", 100)));
        assert_eq!(cache.profile("alice").expect("cached").event_id, "aa");
    }

    #[test]
    fn evict_to_drops_oldest_unpinned() {
        let cache = ProfileCache::new();
        cache.upsert_view("a".into(), view("a", 100));
        cache.upsert_view("b".into(), view("b", 200));
        cache.upsert_view("c".into(), view("c", 300));

        let mut pinned = HashSet::new();
        pinned.insert("a".to_string()); // pinned despite being oldest

        // bring to hwm=2: must remove exactly 1 unpinned, oldest-first → "b"
        let removed = cache.evict_to(&pinned, 2);
        assert_eq!(removed, 1);
        assert!(cache.contains("a"), "pinned survives");
        assert!(!cache.contains("b"), "oldest unpinned reaped");
        assert!(cache.contains("c"));
    }

    #[test]
    fn evict_to_is_noop_at_or_below_hwm() {
        let cache = ProfileCache::new();
        cache.upsert_view("a".into(), view("a", 100));
        assert_eq!(cache.evict_to(&HashSet::new(), 2), 0);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn satisfies_profile_lookup_via_arc_dyn() {
        let cache = Arc::new(ProfileCache::new());
        cache.upsert_view("alice".into(), view("aa", 100));
        let as_trait: Arc<dyn ProfileLookup> = Arc::clone(&cache) as _;
        assert_eq!(as_trait.profile("alice").expect("cached").event_id, "aa");
        assert!(as_trait.contains("alice"));
        assert_eq!(as_trait.len(), 1);
    }
}
