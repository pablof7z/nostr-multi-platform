//! `ProfileLookup` — substrate-generic seam for a per-pubkey kind:0 profile
//! cache.
//!
//! The kernel needs to ask "for author `P`, what is P's cached display name /
//! picture / nip05 / about / lightning address?" when it enriches timeline
//! items, builds profile cards, gates kind:0 re-fetch (TTL / claim dedup),
//! and resolves the zap LNURL. Before ADR-0070 PR 2 this lived in a
//! kernel-owned `profiles: HashMap<String, Profile>` field with a hardwired
//! kind:0 ingest arm (`ingest_profile`) — both NIP-01 nouns inside
//! `nmp-core`. PR 2 moves the cache out behind this trait, mirroring the
//! NIP-17 [`crate::substrate::DmInboxRelayLookup`] migration:
//!
//! - The **writer** is `nmp-nip01`'s `Kind0Parser`
//!   ([`crate::substrate::IngestParser`]) — registered with the kernel's
//!   [`crate::substrate::EventIngestDispatcher`] at composition time. It owns
//!   the supersession rule (newest kind:0 wins, lexicographic event-id
//!   tiebreak) the kernel's `ingest_profile` used to enforce.
//! - The **reader** is the kernel (`profile_for_pubkey`, the claim/TTL gate,
//!   the zap LNURL resolver, RAM eviction) — it consults this trait through a
//!   substrate-generic shape and never names the kind:0 wire format (D0).
//!
//! Both ends agree on a shared `Arc` (the concrete `nmp_nip01::ProfileCache`)
//! at composition time; the kernel sees it only as `Arc<dyn ProfileLookup>`.
//! A kernel built without any profile backend uses [`EmptyProfileLookup`],
//! which reports an always-empty cache (every enrichment field stays `None`,
//! exactly the "no kind:0 has arrived" branch).

use std::sync::Arc;

use serde_json::{Map, Value};

/// Protocol-neutral view of a cached kind:0 profile.
///
/// Carries the raw kind:0 fields the kernel projects, with `Option`/empty-string
/// semantics matching aim.md §2 (`None` / `""` signals "no kind:0 has arrived
/// for this field" — presentation layers own all fallback). The trait hands
/// this out by value so the backing store can use interior mutability without
/// leaking a borrow into the kernel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileView {
    /// Source kind:0 event id (used by the supersession tiebreak + diagnostics).
    pub event_id: String,
    /// Source kind:0 `created_at` (Unix seconds). Drives both supersession
    /// (newest wins) and the RAM-eviction LRU ordering.
    pub created_at: u64,
    /// Verbatim display-name value (`display_name` / `displayName` / `name`,
    /// first non-empty wins). Empty string when the metadata carried none.
    pub display: String,
    /// Raw `name` field from kind:0, when present.
    pub name: Option<String>,
    /// Raw snake-case `display_name` field from kind:0, when present.
    pub raw_display_name: Option<String>,
    /// Raw camel-case `displayName` field from kind:0, when present.
    pub display_name_camel: Option<String>,
    /// Raw picture URL, or `None` when absent / not `http`-prefixed.
    pub picture_url: Option<String>,
    /// Raw `banner` field from kind:0, when present.
    pub banner: Option<String>,
    /// Raw `website` field from kind:0, when present.
    pub website: Option<String>,
    /// NIP-05 identifier, empty string when absent.
    pub nip05: String,
    /// About / bio text, empty string when absent.
    pub about: String,
    /// Raw NIP-57 lightning address (`lud16`), when present.
    pub lud16: Option<String>,
    /// Raw NIP-57 LNURL-pay value (`lud06`), when present.
    pub lud06: Option<String>,
    /// NIP-57 lightning address (`lud16`) or LNURL (`lud06`), `None` when
    /// neither is present (or both are empty).
    pub lnurl: Option<String>,
    /// Full kind:0 JSON object, used only inside Rust to preserve unknown
    /// third-party fields when publishing profile edits.
    pub raw_fields: Map<String, Value>,
}

/// Lookup + eviction contract over a per-pubkey kind:0 profile cache.
///
/// Implementations MUST use interior mutability for the backing store — every
/// method takes `&self`. The writer side (the kind:0 ingest parser) drives
/// supersession through a different method on the concrete type.
pub trait ProfileLookup: Send + Sync {
    /// Resolve `pubkey`'s cached profile, or `None` when no kind:0 is cached.
    /// `pubkey` is lowercase hex.
    fn profile(&self, pubkey: &str) -> Option<ProfileView>;

    /// `true` iff a kind:0 is cached for `pubkey`. Cheaper than [`Self::profile`]
    /// for the claim/TTL membership gates that only need presence.
    fn contains(&self, pubkey: &str) -> bool;

    /// Number of cached profiles. Diagnostic + RAM-eviction watermark check.
    fn len(&self) -> usize;

    /// `true` iff no profile is cached.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Estimated heap bytes held by the cached profiles, for the
    /// `estimated_store_bytes` diagnostic. The cache owns the per-entry byte
    /// accounting for its own value type (the kernel can no longer iterate the
    /// values directly). Returns the same formula the kernel's former
    /// `compute_estimated_store_bytes` profile term used.
    fn estimated_bytes(&self) -> usize;

    /// Pin-aware RAM eviction (#1088 RAM tier). Bring the cache down to `hwm`
    /// entries by removing the oldest (lowest `created_at`, lexicographic
    /// pubkey tiebreak) entries whose pubkey is NOT in `pinned`. Returns the
    /// number of entries removed.
    ///
    /// The kernel computes `pinned` (followed authors + claimed profiles +
    /// active account + open-interest authors) and passes it in — the cache
    /// owns the mechanism, the kernel owns the policy. A no-op when the cache
    /// already sits at/below `hwm`.
    fn evict_to(&self, pinned: &std::collections::HashSet<String>, hwm: usize) -> usize;
}

/// Default backing — the kernel cold-start cache. Always empty; every
/// `profile` lookup returns `None` (the "no kind:0 has arrived" branch the
/// projection builders already expect).
#[derive(Default)]
pub struct EmptyProfileLookup;

impl ProfileLookup for EmptyProfileLookup {
    fn profile(&self, _pubkey: &str) -> Option<ProfileView> {
        None
    }
    fn contains(&self, _pubkey: &str) -> bool {
        false
    }
    fn len(&self) -> usize {
        0
    }
    fn estimated_bytes(&self) -> usize {
        0
    }
    fn evict_to(&self, _pinned: &std::collections::HashSet<String>, _hwm: usize) -> usize {
        0
    }
}

/// Convenience: a fresh `Arc<dyn ProfileLookup>` backed by
/// [`EmptyProfileLookup`] — the kernel's default before a profile cache is
/// wired in.
#[must_use]
pub fn empty_profile_lookup() -> Arc<dyn ProfileLookup> {
    Arc::new(EmptyProfileLookup)
}

/// Test-only in-memory lookup for the substrate `ProfileLookup` trait.
///
/// Production composition uses `nmp_nip01::ProfileCache`. This seed-only double
/// lives inside `nmp-core` so core tests can exercise profile-enrichment /
/// claim-TTL / zap-LNURL / RAM-eviction readers without depending on, parsing,
/// or mirroring NIP-01 kind:0 semantics.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct TestProfileLookup {
    inner: std::sync::RwLock<std::collections::HashMap<String, ProfileView>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestProfileLookup {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a pre-built [`ProfileView`] under `pubkey`.
    ///
    /// This intentionally does not parse kind:0 JSON or enforce NIP-01
    /// supersession. Parser/cache semantics belong to `nmp-nip01`; core tests use
    /// this only to populate the reader seam with already-owned values.
    pub fn seed_view(&self, pubkey: &str, view: ProfileView) {
        let Ok(mut guard) = self.inner.write() else {
            return;
        };
        guard.insert(pubkey.to_string(), view);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ProfileLookup for TestProfileLookup {
    fn profile(&self, pubkey: &str) -> Option<ProfileView> {
        self.inner.read().ok().and_then(|g| g.get(pubkey).cloned())
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
        self.inner
            .read()
            .map(|g| {
                g.values()
                    .map(|p| {
                        p.event_id.len()
                            + p.display.len()
                            + p.name.as_ref().map_or(0, String::len)
                            + p.raw_display_name.as_ref().map_or(0, String::len)
                            + p.display_name_camel.as_ref().map_or(0, String::len)
                            + p.picture_url.as_ref().map_or(0, String::len)
                            + p.banner.as_ref().map_or(0, String::len)
                            + p.website.as_ref().map_or(0, String::len)
                            + p.nip05.len()
                            + p.about.len()
                            + p.lud16.as_ref().map_or(0, String::len)
                            + p.lud06.as_ref().map_or(0, String::len)
                            + p.raw_fields
                                .iter()
                                .map(|(k, v)| k.len() + v.to_string().len())
                                .sum::<usize>()
                            + 96
                    })
                    .sum()
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_lookup_is_empty() {
        let lookup: Arc<dyn ProfileLookup> = empty_profile_lookup();
        assert!(lookup.profile("alice").is_none());
        assert!(!lookup.contains("alice"));
        assert!(lookup.is_empty());
        assert_eq!(lookup.len(), 0);
        assert_eq!(lookup.evict_to(&HashSet::new(), 0), 0);
    }

    #[test]
    fn dyn_trait_object_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        let lookup: Arc<dyn ProfileLookup> = empty_profile_lookup();
        assert_send_sync(lookup);
    }
}
