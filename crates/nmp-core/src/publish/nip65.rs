//! `Nip65OutboxResolver` — concrete `OutboxResolver` impl reading kind:10002
//! relay lists from an `EventStore`.
//!
//! Per NIP-65:
//! - kind:10002 events carry `["r", <url>, <marker?>]` tags where `<marker?>`
//!   is one of `"read"` / `"write"` (absent ⇒ both).
//! - For a publish authored by `A` with `#p` recipients `R1..Rn`:
//!   - resolve write-relays of `A`
//!   - union read-relays of each `Ri`
//!   - if `A` has no kind:10002, fall back to a configurable indexer set.
//!
//! D3 (outbox automatic): callers pass `PublishTarget::Auto`; this resolver
//! picks relays from durable state, never from a hardcoded constant — the
//! indexer fallback is the seam through which the kernel injects a bootstrap
//! relay set, not a policy choice.
//!
//! D7 (capabilities report): bad-shape kind:10002 tags (missing url, non-wss)
//! are logged via `tracing::debug!` and skipped — never crash; never return an
//! exception across the resolver boundary.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::store::{EventStore, PubKey, StoredEvent};

use super::action::{PublishTarget, RelayUrl};
use super::traits::OutboxResolver;

/// Default indexer bootstrap relays for `Nip65OutboxResolver` when the author
/// has no kind:10002 on file. Per the e2e validation spec, two large public
/// strfry relays — neither is authoritative, both are best-effort.
pub const DEFAULT_INDEXER_FALLBACK: &[&str] =
    &["wss://relay.damus.io", "wss://nos.lol"];

/// Resolve `PublishTarget::Auto` to a concrete relay set per NIP-65, using an
/// `EventStore` as the source of truth for kind:10002 lookups.
///
/// `indexer_fallback` is consulted only when the author has no kind:10002 (or
/// when the lookup fails). Recipient `#p` reads are unioned in regardless.
pub struct Nip65OutboxResolver {
    store: Arc<dyn EventStore>,
    indexer_fallback: Vec<RelayUrl>,
}

impl Nip65OutboxResolver {
    /// Build a resolver over the given store. `indexer_fallback` should be the
    /// (small) bootstrap set the kernel falls back to when an author hasn't
    /// published kind:10002 yet. Typically `DEFAULT_INDEXER_FALLBACK`.
    pub fn new(store: Arc<dyn EventStore>, indexer_fallback: Vec<RelayUrl>) -> Self {
        Self {
            store,
            indexer_fallback,
        }
    }

    /// Build a resolver with the default indexer fallback.
    pub fn with_default_fallback(store: Arc<dyn EventStore>) -> Self {
        Self::new(
            store,
            DEFAULT_INDEXER_FALLBACK
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        )
    }

    /// Look up the latest kind:10002 for `author_hex` and parse it into
    /// `(write_relays, read_relays)`. `(both, both)` is the unmarked case.
    fn lookup_kind10002(&self, author_hex: &str) -> Option<(Vec<RelayUrl>, Vec<RelayUrl>)> {
        let author = hex_to_pubkey(author_hex)?;
        let iter = self
            .store
            .scan_by_author_kind(&author, &[10002], None, None, 1)
            .ok()?;
        let stored = iter.into_iter().next()?.ok()?;
        Some(parse_nip65_tags(&stored))
    }
}

impl OutboxResolver for Nip65OutboxResolver {
    fn resolve(
        &self,
        author_pubkey: &str,
        p_tags: &[String],
        target: &PublishTarget,
    ) -> BTreeSet<RelayUrl> {
        // 1. Explicit targets win — the caller has opted out per D3.
        if let PublishTarget::Explicit { relays } = target {
            return relays.iter().cloned().collect();
        }

        let mut out: BTreeSet<RelayUrl> = BTreeSet::new();

        // 2. Author write-relays.
        match self.lookup_kind10002(author_pubkey) {
            Some((writes, _reads)) if !writes.is_empty() => {
                out.extend(writes);
            }
            // Either no kind:10002 or kind:10002 has no write-marker tags.
            // Indexer fallback covers both: it's the bootstrap set the kernel
            // injects when we have nothing better.
            _ => {
                out.extend(self.indexer_fallback.iter().cloned());
            }
        }

        // 3. Recipient read-relays — union for every `#p` tag.
        for p in p_tags {
            if let Some((_writes, reads)) = self.lookup_kind10002(p) {
                out.extend(reads);
            }
        }

        out
    }
}

/// Parse a stored kind:10002 event into `(write_relays, read_relays)`.
///
/// Per NIP-65 tag shape: `["r", <url>, <marker?>]` where `<marker?>` ∈
/// `{"read", "write"}`. Absent marker ⇒ both (the relay appears in both
/// returned lists). Malformed tags (missing url, non-wss) are skipped.
fn parse_nip65_tags(stored: &StoredEvent) -> (Vec<RelayUrl>, Vec<RelayUrl>) {
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for tag in &stored.raw.tags {
        if tag.first().map(String::as_str) != Some("r") {
            continue;
        }
        let Some(url) = tag.get(1) else {
            continue;
        };
        if !is_relay_url(url) {
            continue;
        }
        match tag.get(2).map(String::as_str) {
            Some("write") => writes.push(url.clone()),
            Some("read") => reads.push(url.clone()),
            None | Some("") => {
                writes.push(url.clone());
                reads.push(url.clone());
            }
            Some(_other) => {
                // Unknown marker — most clients (Damus, Amethyst) treat
                // unknown markers as "both", per NIP-65's tolerant parsing
                // intent. Mirror that.
                writes.push(url.clone());
                reads.push(url.clone());
            }
        }
    }
    (writes, reads)
}

/// Decode a 64-char lowercase-hex pubkey into a `PubKey` (`[u8; 32]`). Returns
/// `None` on any malformed input — caller treats `None` as "no lookup".
fn hex_to_pubkey(hex: &str) -> Option<PubKey> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn is_relay_url(url: &str) -> bool {
    // Conservative: only accept ws:// or wss://. NIP-65 specifies relay URLs,
    // not HTTP; reject obvious garbage early.
    url.starts_with("wss://") || url.starts_with("ws://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MemEventStore, RawEvent, VerifiedEvent};

    const AUTHOR_HEX: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    const RECIPIENT_HEX: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";

    fn store_kind10002(store: &dyn EventStore, author_hex: &str, tags: Vec<Vec<String>>) {
        // Construct a unique 64-hex id keyed off author + kind so multiple
        // inserts in the same test do not collide.
        let prefix = &author_hex[..2];
        let id = format!("{:0<64}", format!("{}e10002", prefix));
        let raw = RawEvent {
            id,
            pubkey: author_hex.to_string(),
            created_at: 1_700_000_000,
            kind: 10002,
            tags,
            content: String::new(),
            sig: "0".repeat(128),
        };
        let verified = VerifiedEvent::from_raw_unchecked(raw);
        store
            .insert(verified, &"wss://test".to_string(), 1_700_000_000_000)
            .expect("insert");
    }

    fn mk_resolver(store: Arc<dyn EventStore>) -> Nip65OutboxResolver {
        Nip65OutboxResolver::new(
            store,
            vec!["wss://fallback.example".to_string()],
        )
    }

    #[test]
    fn nip65_resolver_uses_author_writes_when_present() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        store_kind10002(
            store.as_ref(),
            AUTHOR_HEX,
            vec![
                vec!["r".into(), "wss://write.example".into(), "write".into()],
                vec!["r".into(), "wss://read.example".into(), "read".into()],
            ],
        );
        let resolver = mk_resolver(store);
        let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto);
        assert!(out.contains("wss://write.example"));
        // Read-only relays are NOT used for the author's own writes.
        assert!(!out.contains("wss://read.example"));
        // Fallback NOT consulted when author has writes.
        assert!(!out.contains("wss://fallback.example"));
    }

    #[test]
    fn nip65_resolver_falls_back_to_indexer_when_no_kind10002() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        let resolver = mk_resolver(store);
        let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto);
        assert_eq!(out.len(), 1);
        assert!(out.contains("wss://fallback.example"));
    }

    #[test]
    fn nip65_resolver_unions_recipient_reads_for_p_tags() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        store_kind10002(
            store.as_ref(),
            AUTHOR_HEX,
            vec![vec!["r".into(), "wss://author-write.example".into(), "write".into()]],
        );
        store_kind10002(
            store.as_ref(),
            RECIPIENT_HEX,
            vec![vec!["r".into(), "wss://recipient-read.example".into(), "read".into()]],
        );
        let resolver = mk_resolver(store);
        let out = resolver.resolve(
            AUTHOR_HEX,
            &[RECIPIENT_HEX.to_string()],
            &PublishTarget::Auto,
        );
        assert!(out.contains("wss://author-write.example"));
        assert!(out.contains("wss://recipient-read.example"));
    }

    #[test]
    fn nip65_resolver_returns_explicit_unchanged() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        let resolver = mk_resolver(store);
        let explicit = vec![
            "wss://a.example".to_string(),
            "wss://b.example".to_string(),
        ];
        let out = resolver.resolve(
            AUTHOR_HEX,
            &[],
            &PublishTarget::Explicit {
                relays: explicit.clone(),
            },
        );
        assert_eq!(out, explicit.into_iter().collect::<BTreeSet<_>>());
    }

    #[test]
    fn nip65_resolver_handles_malformed_kind10002_gracefully() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        store_kind10002(
            store.as_ref(),
            AUTHOR_HEX,
            vec![
                // Missing url tag → skip
                vec!["r".into()],
                // Non-relay scheme → skip
                vec!["r".into(), "https://example.com".into()],
                // Valid one to confirm we don't abort
                vec!["r".into(), "wss://valid.example".into(), "write".into()],
                // Garbage tag prefix → skip
                vec!["x".into(), "wss://wrong-tag.example".into()],
            ],
        );
        let resolver = mk_resolver(store);
        let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto);
        assert!(out.contains("wss://valid.example"));
        assert!(!out.contains("https://example.com"));
        assert!(!out.contains("wss://wrong-tag.example"));
    }

    #[test]
    fn nip65_resolver_unmarked_tag_is_both() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        store_kind10002(
            store.as_ref(),
            AUTHOR_HEX,
            vec![vec!["r".into(), "wss://both.example".into()]],
        );
        store_kind10002(
            store.as_ref(),
            RECIPIENT_HEX,
            vec![vec!["r".into(), "wss://recipient-both.example".into()]],
        );
        let resolver = mk_resolver(store);
        let out = resolver.resolve(
            AUTHOR_HEX,
            &[RECIPIENT_HEX.to_string()],
            &PublishTarget::Auto,
        );
        // Unmarked counts as both → write goes here.
        assert!(out.contains("wss://both.example"));
        // Recipient unmarked also reads here.
        assert!(out.contains("wss://recipient-both.example"));
    }

    #[test]
    fn nip65_resolver_invalid_author_hex_falls_back() {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        let resolver = mk_resolver(store);
        // Short / non-hex author → lookup returns None → fallback.
        let out = resolver.resolve("not-hex", &[], &PublishTarget::Auto);
        assert_eq!(out.len(), 1);
        assert!(out.contains("wss://fallback.example"));
    }
}
