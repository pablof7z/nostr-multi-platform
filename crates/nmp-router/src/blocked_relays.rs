//! `Kind10006Parser` + `InMemoryBlockedRelayCache` — the kind:10006 (NIP-51
//! "blocked relays" list) ingest path and the routing-side cache it feeds.
//!
//! Structural sibling of [`crate::Kind10002Parser`] (NIP-65 kind:10002) and
//! `nmp_nip17::Kind10050Parser` (NIP-17 kind:10050 DM-relay list). The
//! kernel's [`nmp_core::substrate::EventIngestDispatcher`] fans every
//! accepted `Inserted | Replaced` event to every registered parser; this
//! parser filters on `evt.raw().kind == 10006` so an unintended dispatch
//! is a silent no-op rather than corrupting the blocked-relay cache.
//!
//! # Tag shape — NIP-51 § kind:10006
//!
//! ```text
//! ["relay", "<wss-url>"]
//! ```
//!
//! Every `relay` tag is a blocked relay URL. Non-`relay` tags and entries
//! without a URL value are silently dropped. The parser is strict about
//! the scheme: only `wss://` URLs are kept (a `ws://` or `https://` entry
//! is a misconfiguration that would mismatch routing's canonical
//! URL form).
//!
//! # Canonicalisation + dedupe
//!
//! Each URL is canonicalised through `canonicalize_relay_url` (lowercase
//! scheme + host, empty-path trailing slash stripped) so the cache keys
//! match the routing-side wire URL form. Duplicate `relay` tags
//! (post-canonicalisation) are deduped.
//!
//! # Empty-list semantics
//!
//! An accepted kind:10006 carrying zero `relay` tags is the author's "I
//! cleared my blocked-relay list" signal — the cache entry is removed
//! rather than upserted as an empty list. Subsequent
//! [`nmp_core::substrate::BlockedRelayLookup::blocked_relays`] calls for
//! that pubkey fail back to the empty `BlockedRelaySet` default, which
//! the router treats as "no relays blocked" (fail-open — the user
//! explicitly cleared the list, so the prior blocks must NOT linger).
//!
//! # D0 — why this lives in `nmp-router`
//!
//! `nmp-core` is the substrate; it owns the wire-shape-agnostic
//! [`nmp_core::substrate::BlockedRelayLookup`] trait and the
//! [`nmp_core::substrate::BlockedRelaySet`] value type, but it must
//! never name the kind:10006 wire shape (D0 — no NIP-specific nouns
//! in the kernel crate). The concrete cache + the kind:10006 ingest
//! parser live here so the kernel reads through the substrate trait
//! and the cache reads / writes happen in a router-layer module.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use nmp_core::store::VerifiedEvent;
use nmp_core::substrate::{BlockedRelayLookup, BlockedRelaySet, IngestParser};

/// NIP-51 § kind:10006 (blocked relays list) kind number.
const KIND_BLOCKED_RELAYS: u32 = 10_006;

// ─── InMemoryBlockedRelayCache ──────────────────────────────────────────────

/// Per-account blocked-relay cache. Single writer is [`Kind10006Parser`];
/// readers are the kernel's `build_routing_context` snapshot helper (via
/// the [`BlockedRelayLookup`] trait) and any future diagnostic that wants
/// to read the set without the trait shape.
///
/// Lock-poisoning policy mirrors [`crate::cache::InMemoryMailboxCache`]:
/// degrade gracefully on poison (treat as "no data") rather than amplify
/// a background-thread panic into an actor-wide kill switch (D15).
#[derive(Default)]
pub struct InMemoryBlockedRelayCache {
    inner: RwLock<HashMap<String, Vec<String>>>,
}

impl InMemoryBlockedRelayCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert `account_pubkey`'s blocked-relay list. Empty `relays` removes
    /// the entry (matches the trait's "empty list collapses to empty set"
    /// contract; the parser hits this branch on kind:10006 with no
    /// `relay` tags — the "I cleared my list" signal).
    pub fn upsert(&self, account_pubkey: String, relays: Vec<String>) {
        // D15: silently drop the mutation on poison; next successful
        // writer will overwrite.
        let Ok(mut guard) = self.inner.write() else { return };
        if relays.is_empty() {
            guard.remove(&account_pubkey);
        } else {
            guard.insert(account_pubkey, relays);
        }
    }

    /// Number of accounts with a non-empty blocked list. Returns `0` on a
    /// poisoned lock.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl BlockedRelayLookup for InMemoryBlockedRelayCache {
    fn blocked_relays(&self, account_pubkey: &str) -> BlockedRelaySet {
        let mut set = BlockedRelaySet::new();
        if let Ok(guard) = self.inner.read() {
            if let Some(urls) = guard.get(account_pubkey) {
                for url in urls {
                    set.insert(url.clone());
                }
            }
        }
        set
    }
}

// ─── Kind10006Parser ─────────────────────────────────────────────────────────

/// The kind:10006 ingest parser. Constructed with a shared
/// [`std::sync::Arc<InMemoryBlockedRelayCache>`] handle — the same `Arc`
/// the kernel holds as its `Arc<dyn BlockedRelayLookup>` so the writer
/// side (this parser) and the reader side (the kernel's
/// `build_routing_context`) see one source of truth.
pub struct Kind10006Parser {
    cache: std::sync::Arc<InMemoryBlockedRelayCache>,
}

impl Kind10006Parser {
    /// Construct a parser writing into the supplied
    /// [`InMemoryBlockedRelayCache`].
    #[must_use]
    pub fn new(cache: std::sync::Arc<InMemoryBlockedRelayCache>) -> Self {
        Self { cache }
    }

    /// Static-dispatch path for tests and direct callers. Identical effect
    /// to [`IngestParser::parse`]. Returns `false` (no-op) when `evt`'s
    /// kind is not 10006; returns `true` when the event was decoded and
    /// upserted (including the empty-list "clear my list" case).
    pub fn parse_event(&self, evt: &VerifiedEvent) -> bool {
        let raw = evt.raw();
        if raw.kind != KIND_BLOCKED_RELAYS {
            return false;
        }
        let relays = parse_blocked_relay_list(&raw.tags);
        self.cache.upsert(raw.pubkey.clone(), relays);
        true
    }
}

impl IngestParser for Kind10006Parser {
    fn parse(&self, evt: &VerifiedEvent) {
        let _ = self.parse_event(evt);
    }
}

/// Decode the `["relay", <wss-url>]` tags of a kind:10006 event into a
/// deduped, canonicalised blocked relay URL list.
///
/// Non-`relay` tags, `relay` tags with no URL value, and URLs that do not
/// start with `wss://` are silently skipped — the same defensive scheme
/// gate `Kind10002Parser` applies, for the same reason (a non-wss URL in a
/// routing tag is misconfiguration that would not match the wire-routing
/// canonical form).
fn parse_blocked_relay_list(tags: &[Vec<String>]) -> Vec<String> {
    let mut relays = Vec::new();
    let mut seen = HashSet::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("relay") {
            continue;
        }
        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
            continue;
        };
        let canonical = canonicalize_relay_url(url);
        if seen.insert(canonical.clone()) {
            relays.push(canonical);
        }
    }

    relays
}

/// Canonicalise a `wss://` relay URL: lowercase scheme + host, strip the
/// empty-path trailing slash. Identical shape to the helpers in
/// `crate::ingest` (kind:10002) and `nmp_nip17::kind10050_parser` so
/// cache keys across all three NIP-51-adjacent caches collide cleanly.
fn canonicalize_relay_url(url: &str) -> String {
    const PREFIX: &str = "wss://";
    debug_assert!(url.starts_with(PREFIX));
    let rest = &url[PREFIX.len()..];
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let canonical_host = host_port.to_lowercase();
    let canonical_path = if path == "/" { "" } else { path };
    format!("{PREFIX}{canonical_host}{canonical_path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::RawEvent;
    use nmp_core::substrate::EventIngestDispatcher;
    use std::sync::Arc;

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
    fn ignores_non_kind_10006() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));
        let accepted = parser.parse_event(&evt(
            "alice",
            10_002, // wrong kind
            vec![vec!["relay".into(), "wss://blocked.example".into()]],
        ));
        assert!(!accepted, "wrong-kind events must not mutate the cache");
        assert!(cache.is_empty());
    }

    #[test]
    fn well_formed_kind_10006_upserts_blocked_set() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![
                vec!["relay".into(), "wss://block-a.example".into()],
                vec!["relay".into(), "wss://block-b.example".into()],
            ],
        ));

        let resolved = cache.blocked_relays("alice");
        assert!(resolved.contains(&"wss://block-a.example".to_string()));
        assert!(resolved.contains(&"wss://block-b.example".to_string()));
    }

    #[test]
    fn empty_kind_10006_clears_prior_entry() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![vec!["relay".into(), "wss://block.example".into()]],
        ));
        assert!(
            !cache.blocked_relays("alice").is_empty(),
            "precondition: cached"
        );

        parser.parse_event(&evt("alice", 10_006, Vec::new()));
        assert!(
            cache.blocked_relays("alice").is_empty(),
            "empty kind:10006 must remove the entry (fail-open: cleared list \
             is indistinguishable from no list)"
        );
    }

    #[test]
    fn ignores_non_relay_and_non_wss_tags() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![
                // NIP-65 `r` marker — must NOT be read as a blocked relay.
                vec!["r".into(), "wss://nip65.example".into(), "write".into()],
                // Non-wss schemes are misconfiguration.
                vec!["relay".into(), "ws://insecure.example".into()],
                vec!["relay".into(), "https://not-a-relay.example".into()],
                // `relay` tag with no URL value.
                vec!["relay".into()],
                // The one well-formed blocked relay.
                vec!["relay".into(), "wss://valid-block.example".into()],
            ],
        ));

        let resolved = cache.blocked_relays("alice");
        assert!(resolved.contains(&"wss://valid-block.example".to_string()));
        assert!(!resolved.contains(&"wss://nip65.example".to_string()));
        assert!(!resolved.contains(&"ws://insecure.example".to_string()));
        assert!(!resolved.contains(&"https://not-a-relay.example".to_string()));
    }

    #[test]
    fn canonicalizes_host_and_dedupes_duplicates() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));
        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![
                vec!["relay".into(), "wss://Block.Example/".into()],
                vec!["relay".into(), "wss://block.example/".into()],
                vec!["relay".into(), "wss://other.example/".into()],
            ],
        ));

        let resolved = cache.blocked_relays("alice");
        // Canonicalised form: lowercase, trailing-slash stripped.
        assert!(resolved.contains(&"wss://block.example".to_string()));
        assert!(resolved.contains(&"wss://other.example".to_string()));
        // Cache stored exactly two entries — the two mixed-case variants
        // collapsed.
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn newer_kind_10006_replaces_cached_list() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![vec!["relay".into(), "wss://old-block.example".into()]],
        ));
        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![vec!["relay".into(), "wss://new-block.example".into()]],
        ));

        let resolved = cache.blocked_relays("alice");
        assert!(resolved.contains(&"wss://new-block.example".to_string()));
        assert!(!resolved.contains(&"wss://old-block.example".to_string()));
    }

    #[test]
    fn registers_as_ingest_parser_trait_object() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser: Arc<dyn IngestParser> =
            Arc::new(Kind10006Parser::new(Arc::clone(&cache)));

        let mut dispatcher = EventIngestDispatcher::new();
        dispatcher.register_kind(10_006, parser);
        dispatcher.dispatch(&evt(
            "alice",
            10_006,
            vec![vec!["relay".into(), "wss://via.dispatcher".into()]],
        ));

        assert!(cache
            .blocked_relays("alice")
            .contains(&"wss://via.dispatcher".to_string()));
    }

    #[test]
    fn separate_authors_have_isolated_block_sets() {
        let cache = Arc::new(InMemoryBlockedRelayCache::new());
        let parser = Kind10006Parser::new(Arc::clone(&cache));

        parser.parse_event(&evt(
            "alice",
            10_006,
            vec![vec!["relay".into(), "wss://alice-block.example".into()]],
        ));
        parser.parse_event(&evt(
            "bob",
            10_006,
            vec![vec!["relay".into(), "wss://bob-block.example".into()]],
        ));

        // Alice sees her block but not Bob's, and vice versa.
        let alice = cache.blocked_relays("alice");
        assert!(alice.contains(&"wss://alice-block.example".to_string()));
        assert!(!alice.contains(&"wss://bob-block.example".to_string()));

        let bob = cache.blocked_relays("bob");
        assert!(bob.contains(&"wss://bob-block.example".to_string()));
        assert!(!bob.contains(&"wss://alice-block.example".to_string()));
    }

    #[test]
    fn unknown_account_returns_empty_set() {
        let cache = InMemoryBlockedRelayCache::new();
        let set = cache.blocked_relays("never-published");
        assert!(set.is_empty(), "fail-open default: unknown account = no blocks");
    }
}
