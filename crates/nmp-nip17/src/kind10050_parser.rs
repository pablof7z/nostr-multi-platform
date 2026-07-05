//! `Kind10050Parser` — the [`IngestParser`] that decodes kind:10050 events
//! and upserts the resolved DM-inbox relay list into [`DmRelayCache`].
//!
//! Structural sibling of `nmp_router::Kind10002Parser` (NIP-65 kind:10002),
//! but for the NIP-17 § 2 DM-relay-list contract. The kernel's
//! [`nmp_core::substrate::EventIngestDispatcher`] fans every accepted
//! `Inserted | Replaced` event to every registered parser; this parser
//! filters on `evt.raw().kind == 10050` so an unintended dispatch (e.g. a
//! kind:10002 routed through a misregistration) is a silent no-op rather
//! than corrupting the DM cache.
//!
//! # Tag shape — NIP-17 § 2
//!
//! ```text
//! ["relay", "<wss-url>"]
//! ```
//!
//! Unlike NIP-65 kind:10002, kind:10050 has no `read` / `write` / `both`
//! markers — every `relay` tag is a DM-inbox relay. The parser is
//! deliberately strict about the scheme: only `wss://` URLs are kept
//! (a `ws://` or `https://` DM relay is a misconfiguration that would
//! degrade the seal's confidentiality). Empty URLs and non-`relay` tags
//! are silently dropped.
//!
//! # Canonicalisation + dedupe
//!
//! Each URL is canonicalised through [`canonicalize_relay_url`] (lowercase
//! scheme + host, empty-path trailing slash stripped) so the cache keys
//! match the wire-routing form. Duplicate `relay` tags (post-
//! canonicalisation) are deduped, preserving first-seen tag order — the
//! same shape `nmp_router::Kind10002Parser` produces for kind:10002 URLs.
//!
//! # Empty-list semantics
//!
//! An accepted kind:10050 carrying zero `relay` tags is the author's "I
//! cleared my list" signal. `DmRelayCache::upsert` treats an empty
//! `relays` Vec as "remove the entry", so subsequent lookups fail closed
//! exactly as for an author who never published a kind:10050. The
//! gift-wrap publish path treats both branches identically — the only
//! safe answer for a missing list is "do not publish".

use std::sync::Arc;

use nmp_core::substrate::IngestParser;
use nmp_kinds::KIND_DM_RELAY_LIST;
use nmp_store::VerifiedEvent;

use crate::dm_relay_cache::DmRelayCache;

/// The kind:10050 ingest parser. Constructed with a shared
/// [`Arc<DmRelayCache>`] handle — the same `Arc` the kernel holds as its
/// `Arc<dyn DmInboxRelayLookup>` so the writer side (this parser) and the
/// reader side (the kernel's `recipient_dm_relays` / the planner's
/// `#p`-tagged inbox routing) see one source of truth.
pub struct Kind10050Parser {
    cache: Arc<DmRelayCache>,
}

impl Kind10050Parser {
    /// Construct a parser writing into the supplied [`DmRelayCache`].
    #[must_use]
    pub fn new(cache: Arc<DmRelayCache>) -> Self {
        Self { cache }
    }

    /// Static-dispatch path for tests and direct callers. Identical effect
    /// to [`IngestParser::parse`]. Returns `false` (no-op) when `evt`'s
    /// kind is not 10050; returns `true` when the event was decoded and
    /// upserted (including the empty-list "clear my list" case).
    pub fn parse_event(&self, evt: &VerifiedEvent) -> bool {
        let raw = evt.raw();
        if raw.kind != KIND_DM_RELAY_LIST {
            return false;
        }
        let relays = parse_dm_relay_list(&raw.tags);
        // #3071: pass the event's `created_at` so the cache keeps the NEWEST
        // kind:10050 per author — a stale one (accumulated across sessions or
        // replayed last on cold relaunch) must not overwrite the current list.
        self.cache.upsert(raw.pubkey.clone(), raw.created_at, relays);
        true
    }
}

impl IngestParser for Kind10050Parser {
    fn parse(&self, evt: &VerifiedEvent) {
        let _ = self.parse_event(evt);
    }
}

/// Decode the `["relay", <wss-url>]` tags of a kind:10050 event into a
/// deduped, canonicalised DM-inbox relay URL list.
///
/// Non-`relay` tags, `relay` tags with no URL value, and URLs that do not
/// start with `wss://` are silently skipped — the same defensive scheme
/// gate the legacy kernel-side parser applied. The preserved first-seen
/// tag order matches `nmp_router::Kind10002Parser`'s behaviour for the
/// NIP-65 path, so the kind:10002 + kind:10050 round-trip shapes line up.
fn parse_dm_relay_list(tags: &[Vec<String>]) -> Vec<String> {
    let mut relays = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("relay") {
            continue;
        }
        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
            continue;
        };
        // Fail-closed: drop any URL the canonical authority rejects (a bare
        // `wss://` / `wss:///path` with no host) rather than admitting a
        // malformed key into the DM-inbox cache (#967).
        let Some(canonical) = canonicalize_relay_url(url) else {
            continue;
        };
        if seen.insert(canonical.clone()) {
            relays.push(canonical);
        }
    }

    relays
}

/// Canonicalise a `wss://` relay URL through the single workspace authority
/// [`nmp_core::substrate::canonicalize_relay_url`] (lowercase scheme + host,
/// strip the empty-path trailing slash). Returns `None` (fail-closed) for an
/// off-contract URL so a malformed `relay` tag is dropped, not stored. The
/// rules are NOT re-implemented here — converged onto the one authority (#967).
fn canonicalize_relay_url(url: &str) -> Option<String> {
    debug_assert!(url.starts_with("wss://"));
    nmp_core::substrate::canonicalize_relay_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventIngestDispatcher;
    use nmp_store::RawEvent;

    fn evt(pubkey: &str, kind: u32, tags: Vec<Vec<String>>) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: "00".repeat(32),
            pubkey: pubkey.into(),
            created_at: 0,
            kind,
            tags,
            content: String::new(),
            sig: "22".repeat(64),
        })
    }

    #[test]
    fn ignores_non_kind_10050() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));
        let accepted = parser.parse_event(&evt(
            "alice",
            10_002, // wrong kind
            vec![vec!["relay".into(), "wss://dm.example".into()]],
        ));
        assert!(!accepted, "wrong-kind events must not mutate the cache");
        assert!(cache.is_empty());
    }

    #[test]
    fn well_formed_kind_10050_upserts_relays() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![
                vec!["relay".into(), "wss://dm-a.example".into()],
                vec!["relay".into(), "wss://dm-b.example".into()],
            ],
        ));

        let resolved = cache.read_relays("alice").expect("alice's list populates");
        assert_eq!(
            resolved,
            vec!["wss://dm-a.example", "wss://dm-b.example"],
            "every `relay` tag is a DM-inbox relay, in tag order"
        );
    }

    #[test]
    fn empty_kind_10050_clears_prior_entry() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![vec!["relay".into(), "wss://dm.example".into()]],
        ));
        assert!(cache.read_relays("alice").is_some(), "precondition: cached");

        parser.parse_event(&evt("alice", 10_050, Vec::new()));
        assert!(
            cache.read_relays("alice").is_none(),
            "an empty kind:10050 removes the stale entry (NIP-17 § 2 fail-closed)"
        );
    }

    #[test]
    fn ignores_non_relay_and_non_wss_tags() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![
                // NIP-65 `r` marker — must NOT be read as a DM relay.
                vec!["r".into(), "wss://nip65.example".into(), "write".into()],
                // Non-wss schemes are rejected (no plaintext DM transport).
                vec!["relay".into(), "ws://insecure.example".into()],
                vec!["relay".into(), "https://not-a-relay.example".into()],
                // A `relay` tag with no URL value.
                vec!["relay".into()],
                // The one well-formed DM relay.
                vec!["relay".into(), "wss://valid.example".into()],
            ],
        ));

        let resolved = cache
            .read_relays("alice")
            .expect("the one well-formed tag must land");
        assert_eq!(
            resolved,
            vec!["wss://valid.example"],
            "only well-formed wss `relay` tags survive"
        );
    }

    #[test]
    fn canonicalizes_host_and_dedupes_duplicates() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![
                vec!["relay".into(), "wss://DM-Relay.Example/".into()],
                // Mixed-case duplicate — must dedupe post-canonicalisation.
                vec!["relay".into(), "wss://dm-relay.example/".into()],
                vec!["relay".into(), "wss://other.example/".into()],
            ],
        ));

        let resolved = cache.read_relays("alice").expect("alice's list resolves");
        assert_eq!(
            resolved,
            vec!["wss://dm-relay.example", "wss://other.example"],
            "lowercase host + trailing-slash strip + dedupe"
        );
    }

    #[test]
    fn newer_kind_10050_replaces_cached_list() {
        let cache = Arc::new(DmRelayCache::new());
        let parser = Kind10050Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![vec!["relay".into(), "wss://old-dm.example".into()]],
        ));
        parser.parse_event(&evt(
            "alice",
            10_050,
            vec![vec!["relay".into(), "wss://new-dm.example".into()]],
        ));

        assert_eq!(
            cache.read_relays("alice"),
            Some(vec!["wss://new-dm.example".to_string()]),
            "the newer kind:10050 must replace the cached DM-relay list"
        );
    }

    #[test]
    fn registers_as_ingest_parser_trait_object() {
        // Compile-check the IngestParser shape — confirms the trait is
        // satisfied so `EventIngestDispatcher::register_kind` accepts it.
        let cache = Arc::new(DmRelayCache::new());
        let parser: Arc<dyn IngestParser> = Arc::new(Kind10050Parser::new(Arc::clone(&cache)));

        let mut dispatcher = EventIngestDispatcher::new();
        dispatcher.register_kind(10_050, parser);
        dispatcher.dispatch(&evt(
            "alice",
            10_050,
            vec![vec!["relay".into(), "wss://via.dispatcher".into()]],
        ));

        assert_eq!(
            cache.read_relays("alice"),
            Some(vec!["wss://via.dispatcher".to_string()]),
        );
    }

    #[test]
    fn canonicalize_relay_url_preserves_explicit_paths() {
        assert_eq!(
            canonicalize_relay_url("wss://Host.Example/").as_deref(),
            Some("wss://host.example"),
        );
        assert_eq!(
            canonicalize_relay_url("wss://Host.Example/some/path").as_deref(),
            Some("wss://host.example/some/path"),
            "explicit paths are preserved verbatim",
        );
        assert_eq!(
            canonicalize_relay_url("wss://host.example:8443/").as_deref(),
            Some("wss://host.example:8443"),
            "ports survive canonicalisation",
        );
    }
}
