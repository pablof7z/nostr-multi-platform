//! `AdUrlResolver` — renderer→host bridge for NIP-AD URL resolution (#2927).
//!
//! Mirrors [`EventRefResolver`](super::event_ref_resolver::EventRefResolver)
//! exactly, one level up: where `EventRefResolver` claims an already-known
//! event pointer (`nevent`/`naddr`), `AdUrlResolver` claims a plain `http(s)`
//! URL that MIGHT double as a NIP-AD pointer. The trait lives in `nmp-content`
//! so renderers (e.g. `NostrContentView`) can take `Option<&dyn AdUrlResolver>`
//! without `nmp-content` ever gaining an `nmp-nip-ad` / `nmp-native-runtime`
//! dependency; each platform host supplies the impl that bridges to the
//! resolver + the `open_ad_collection` delivery doorway.
//!
//! # Fail-open (D1/D6)
//!
//! A [`WireNode::AdCandidateUrl`](crate::wire::WireNode::AdCandidateUrl) ALWAYS
//! renders as a plain link immediately — claiming is a strictly-later,
//! non-blocking upgrade, never a precondition for render. Most URLs are not
//! AD-enabled; the overwhelmingly common terminal state is
//! [`AdUrlState::ResolutionFailed`], which keeps the plain link. The renderer
//! never waits: it consults [`AdUrlState`] and renders the link until (and
//! unless) a [`AdUrlState::Resolved`] collection is available.
//!
//! # Policy is host-owned, not here
//!
//! WHETHER a candidate auto-resolves at render time is the app's call
//! (`nmp_nip_ad::AdResolutionPolicy`, injected at the composition root). This
//! seam stays noun-free: it hands the host the note's `author` so the host can
//! consult its injected policy before firing a resolve. `nmp-content` never
//! imports the policy trait.

/// Host-side bridge that lets a content renderer attempt NIP-AD resolution of a
/// plain `http(s)` URL (#2927). The trait lives in `nmp-content` so the crate
/// never gains an `nmp-nip-ad` / `nmp-native-runtime` dependency; each platform
/// host supplies the impl that (policy-permitting) drives the `.well-known`
/// resolve + `open_ad_collection` delivery.
///
/// URL parsing, the `AdResolutionPolicy` gate, the SSRF-guarded fetch, and the
/// relay-pinned collection open are ALL host-side; the renderer boundary
/// receives only the raw URL string, the note author (so the host can gate),
/// and a refcount consumer id.
///
/// # Examples
///
/// ```
/// use nmp_content::AdUrlResolver;
///
/// struct MyHost;
/// impl AdUrlResolver for MyHost {
///     fn claim_ad_url(&self, _url: &str, _author_pubkey_hex: &str, _consumer_id: &str) {
///         /* policy-gate, then resolve + open_ad_collection */
///     }
///     fn release_ad_url(&self, _url: &str, _consumer_id: &str) { /* refcount-release */ }
/// }
/// let _: Box<dyn AdUrlResolver> = Box::new(MyHost);
/// ```
pub trait AdUrlResolver: Send + Sync {
    /// Attempt (or refcount-increment) NIP-AD resolution of `url` on behalf of
    /// `consumer_id`, rendering a note authored by `author_pubkey_hex` (raw
    /// 32-byte hex). The host consults its injected `AdResolutionPolicy` with
    /// `{author, url}` and, only if permitted, drives the `.well-known` resolve
    /// and `open_ad_collection`.
    ///
    /// Implementations MUST be idempotent and infallible — failure is swallowed
    /// silently (D6) so renderers can call this on every render pass without
    /// guarding, and the plain link keeps rendering regardless.
    fn claim_ad_url(&self, url: &str, author_pubkey_hex: &str, consumer_id: &str);

    /// Release a previously-claimed `(url, consumer_id)` pair. A double-release
    /// or unknown pair is a no-op.
    fn release_ad_url(&self, url: &str, consumer_id: &str);
}

/// No-op sink — fixture/test surfaces and apps that ship the
/// `NeverAutoResolve` posture use this so renderers can run without any AD
/// resolution wiring. Every `AdCandidateUrl` then renders as a permanent plain
/// link (which is the correct baseline).
pub struct NoopAdUrlResolver;

impl AdUrlResolver for NoopAdUrlResolver {
    fn claim_ad_url(&self, _url: &str, _author_pubkey_hex: &str, _consumer_id: &str) {}
    fn release_ad_url(&self, _url: &str, _consumer_id: &str) {}
}

/// Per-URL NIP-AD resolution state (#2927) — the first-class **terminal
/// failure** state the embed pipeline otherwise lacks.
///
/// `EmbedClaimRegistry`'s `Entry.resolved: Option<ResolvedEvent>` models only
/// "not yet" (`None`, waits forever) vs "resolved" (`Some`) — correct for
/// `nevent`/`naddr`, where the pointer is known-good and eventual arrival is
/// the only outcome. AD inverts that: MOST URLs are not AD-enabled and
/// resolution will never succeed, so "wait forever" is wrong. This enum makes
/// "give up and keep the plain link" a first-class, renderable outcome
/// (fail-open).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdUrlState {
    /// Fresh candidate — no resolution attempted yet. Renders as a plain link.
    NotAttempted,
    /// `.well-known/nostr.json?ad=<path>` fetch (and/or the relay-pinned
    /// collection query) is in flight. Still renders as a plain link — the
    /// resolve never blocks render.
    Resolving,
    /// Terminal-until-TTL failure: 404 / timeout / malformed / non-AD domain /
    /// SSRF-guard reject / unresolvable filter. The plain link stays; nothing
    /// is surfaced to the user (D6). `at` is the Unix-seconds timestamp of the
    /// failure so a host cache can honour a negative TTL before re-attempting.
    ResolutionFailed {
        /// Unix-seconds timestamp the failure was recorded.
        at: u64,
    },
    /// Resolved to a live collection (0..N events) delivered under
    /// `projection_key` by `nmp_nip_ad::open_ad_collection`. The host reads the
    /// typed `AdCollectionSnapshot` from its projection registry by this key
    /// and renders each row per-`kind` through the existing
    /// [`resolve_embed_projection`](crate::resolve_embed_projection) pipeline
    /// (kind:30023 article, kind:20 image gallery, …). Carrying the key rather
    /// than the events keeps `nmp-content` decoupled from `nmp-nip-ad`
    /// (read-door doctrine: rendering reads a typed projection snapshot).
    Resolved {
        /// The `nmp_nip_ad::ad_collection_projection_key(session_id)` under
        /// which the delivered collection snapshot is installed.
        projection_key: String,
    },
}

impl AdUrlState {
    /// True while the URL should render as a plain link (every state except a
    /// delivered collection). The single predicate a renderer needs: "do I
    /// still draw the bare link, or the resolved embed?"
    #[must_use]
    pub fn renders_as_plain_link(&self) -> bool {
        !matches!(self, AdUrlState::Resolved { .. })
    }

    /// The delivered collection's projection key, if resolved.
    #[must_use]
    pub fn resolved_projection_key(&self) -> Option<&str> {
        match self {
            AdUrlState::Resolved { projection_key } => Some(projection_key),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_resolver_is_infallible_and_idempotent() {
        let r = NoopAdUrlResolver;
        // Call repeatedly — never panics, never observable.
        r.claim_ad_url("https://example.com/x", "ab".repeat(32).as_str(), "c1");
        r.claim_ad_url("https://example.com/x", "ab".repeat(32).as_str(), "c1");
        r.release_ad_url("https://example.com/x", "c1");
        r.release_ad_url("https://example.com/x", "c1");
    }

    #[test]
    fn fail_open_states_render_as_plain_link() {
        // Every non-Resolved state keeps the bare link (fail-open, never hang).
        assert!(AdUrlState::NotAttempted.renders_as_plain_link());
        assert!(AdUrlState::Resolving.renders_as_plain_link());
        assert!(AdUrlState::ResolutionFailed { at: 100 }.renders_as_plain_link());
        let resolved = AdUrlState::Resolved {
            projection_key: "projection.nmp.nip-ad.collection.s1".to_string(),
        };
        assert!(!resolved.renders_as_plain_link());
        assert_eq!(
            resolved.resolved_projection_key(),
            Some("projection.nmp.nip-ad.collection.s1")
        );
    }

    #[test]
    fn non_resolved_states_have_no_projection_key() {
        assert_eq!(AdUrlState::NotAttempted.resolved_projection_key(), None);
        assert_eq!(
            AdUrlState::ResolutionFailed { at: 1 }.resolved_projection_key(),
            None
        );
    }
}
