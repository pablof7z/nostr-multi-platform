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
mod tests;
