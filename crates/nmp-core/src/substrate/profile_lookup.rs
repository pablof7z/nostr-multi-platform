//! `ProfileLookup` — substrate-generic seam for a per-pubkey kind:0 profile
//! cache.
//!
//! The kernel needs to ask "for author `P`, what is P's cached display name /
//! picture / nip05 / about / lightning address?" when it enriches timeline
//! items, builds profile cards, gates kind:0 re-fetch (TTL / claim dedup),
//! and resolves the zap LNURL. Before ADR-0057 PR 2 this lived in a
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

/// Test-only in-memory cache for the substrate `ProfileLookup` trait.
///
/// Production composition uses `nmp_nip01::ProfileCache`. This stand-in lives
/// inside `nmp-core` so the crate's own tests (which cannot depend on
/// `nmp-nip01`) can still exercise the profile-enrichment / claim-TTL /
/// zap-LNURL / RAM-eviction readers end-to-end. Mirrors the production cache's
/// shape, supersession rule, eviction ordering, and parse contract so a test
/// that seeds through this helper produces the same cache shape the production
/// `Kind0Parser` would.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct TestProfileCache {
    inner: std::sync::RwLock<std::collections::HashMap<String, ProfileView>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestProfileCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert a pre-built [`ProfileView`] under `pubkey`, applying the kind:0
    /// supersession rule (newest `created_at` wins; lexicographically-smaller
    /// event-id wins on a tie). Returns `true` iff the candidate replaced the
    /// cached entry. Mirrors `nmp_nip01::ProfileCache::upsert_view`.
    pub fn upsert_view(&self, pubkey: &str, candidate: ProfileView) -> bool {
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

    /// Parse a kind:0 `content` JSON object + `(event_id, created_at)` into a
    /// [`ProfileView`] and upsert it. The parse contract is a verbatim port of
    /// the production `nmp_nip01::Kind0Parser` (and the kernel's former
    /// `parse_profile`) so test-seeded profiles match production exactly.
    /// Returns whether the candidate superseded the cached entry.
    pub fn ingest_kind0(
        &self,
        pubkey: &str,
        event_id: &str,
        created_at: u64,
        content: &str,
    ) -> bool {
        self.upsert_view(pubkey, parse_kind0_content(event_id, created_at, content))
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ProfileLookup for TestProfileCache {
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

/// Test-only kind:0 [`IngestParser`] that writes a [`TestProfileCache`].
///
/// Production composition registers `nmp_nip01::Kind0Parser`; this stand-in
/// lets `nmp-core`'s own tests exercise the real chokepoint path
/// (`verify_and_persist` → `EventIngestDispatcher` → parser) — so a local
/// kind:0 publish gets read-your-writes through the SAME dispatcher fan-out
/// production uses, without depending on `nmp-nip01`. Registered on the test
/// kernel's dispatcher at construction (mirroring how the production parser is
/// registered by `nmp_defaults::register_substrate`).
#[cfg(any(test, feature = "test-support"))]
pub struct TestKind0Parser {
    cache: std::sync::Arc<TestProfileCache>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestKind0Parser {
    #[must_use]
    pub fn new(cache: std::sync::Arc<TestProfileCache>) -> Self {
        Self { cache }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl crate::substrate::IngestParser for TestKind0Parser {
    fn parse(&self, evt: &crate::store::VerifiedEvent) {
        let raw = evt.raw();
        if raw.kind != 0 {
            return;
        }
        self.cache
            .ingest_kind0(&raw.pubkey, &raw.id, raw.created_at, &raw.content);
    }
}

/// Verbatim port of the production kind:0 parse contract
/// (`nmp_nip01::Kind0Parser` / the kernel's former `parse_profile`). Used by
/// [`TestProfileCache::ingest_kind0`] so `nmp-core`'s own tests produce the
/// same cache shape as production without depending on `nmp-nip01`.
#[cfg(any(test, feature = "test-support"))]
fn parse_kind0_content(event_id: &str, created_at: u64, content: &str) -> ProfileView {
    let raw_fields = serde_json::from_str::<Map<String, Value>>(content).unwrap_or_default();
    let name = string_field(&raw_fields, "name");
    let raw_display_name = string_field(&raw_fields, "display_name");
    let display_name_camel = string_field(&raw_fields, "displayName");
    let picture = string_field(&raw_fields, "picture");
    let nip05 = string_field(&raw_fields, "nip05");
    let about = string_field(&raw_fields, "about");
    let lud16 = string_field(&raw_fields, "lud16");
    let lud06 = string_field(&raw_fields, "lud06");
    let display = raw_display_name
        .clone()
        .or_else(|| display_name_camel.clone())
        .or_else(|| name.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    ProfileView {
        event_id: event_id.to_string(),
        created_at,
        display,
        name,
        raw_display_name,
        display_name_camel,
        picture_url: picture.filter(|value| value.starts_with("http")),
        banner: string_field(&raw_fields, "banner"),
        website: string_field(&raw_fields, "website"),
        nip05: nip05.unwrap_or_default(),
        about: about.unwrap_or_default(),
        lud16: lud16.clone(),
        lud06: lud06.clone(),
        lnurl: lud16
            .filter(|s| !s.trim().is_empty())
            .or_else(|| lud06.filter(|s| !s.trim().is_empty())),
        raw_fields,
    }
}

#[cfg(any(test, feature = "test-support"))]
fn string_field(raw_fields: &Map<String, Value>, key: &str) -> Option<String> {
    raw_fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
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
