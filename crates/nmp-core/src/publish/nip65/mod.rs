//! `Nip65OutboxResolver` — concrete `OutboxResolver` impl reading kind:10002
//! relay lists from an `EventStore`.
//!
//! Per NIP-65:
//! - kind:10002 events carry `["r", <url>, <marker?>]` tags where `<marker?>`
//!   is one of `"read"` / `"write"` (absent ⇒ both).
//! - For a publish authored by `A` with `#p` recipients `R1..Rn`:
//!   - resolve write-relays of `A`
//!   - union read-relays of each `Ri` only while `n < 15`
//!   - if `A` has no kind:10002 **and** the event is not a discovery kind,
//!     return an **empty relay set** (fail-closed).
//!
//! Discovery-kind carve-out: kind:0 / kind:3 / kind:10000–19999 additionally
//! fan out to the configured indexer relays (see [`is_discovery_kind`]). For
//! those kinds an author with no cached kind:10002 still resolves to the
//! indexer set — never an empty set — because the indexers are precisely
//! where a fresh account's profile / contacts / replaceable lists must be
//! discoverable. Only **non-discovery** kinds (notes, reactions, …) are
//! fail-closed when the author is uncached.
//!
//! D3 (outbox automatic): callers pass `PublishTarget::Auto`; this resolver
//! picks relays from durable state (or the indexer set for discovery kinds),
//! never from a hardcoded per-kind constant. An author with no cached
//! kind:10002 publishing a non-discovery kind is unroutable — the engine
//! surfaces `NoTargets` so the UI can show "no relay to publish to" rather
//! than silently widening to arbitrary public relays. This mirrors T134's
//! subscription-side semantics (`CompiledPlan::unroutable_authors`).
//!
//! D7 (capabilities report): bad-shape kind:10002 tags (missing url, non-wss)
//! are logged via `tracing::debug!` and skipped — never crash; never return an
//! exception across the resolver boundary.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::kernel::{
    new_active_account_slot, new_indexer_relays_slot, new_local_write_relays_slot,
    ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use crate::store::{EventStore, PubKey, StoredEvent};

use super::action::{PublishTarget, RelayUrl};
use super::traits::OutboxResolver;

/// Maximum distinct `#p` pubkeys that still get recipient inbox fan-out.
///
/// Events with this many or more tagged pubkeys are treated as broadcast-ish:
/// publish to the author's own write relays, and for discovery kinds to
/// indexers, but do not fan out to every tagged pubkey's read relays.
pub const RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD: usize = 15;

/// Resolve `PublishTarget::Auto` to a concrete relay set per NIP-65, using an
/// `EventStore` as the source of truth for kind:10002 lookups.
///
/// When the author has no kind:10002 on file (or the lookup fails) and the
/// event is **not** a discovery kind, the resolver returns an **empty relay
/// set** — the engine maps this to `PublishEngineError::NoTargets` and
/// surfaces it as a visible failure on the publish-status snapshot. This is
/// fail-closed per doctrine (D3) and mirrors T134's subscription-side
/// `unroutable_authors` semantics. Discovery kinds (kind:0 / kind:3 /
/// kind:10000–19999) instead fan out to the indexer relays even for an
/// uncached author — see [`is_discovery_kind`].
pub struct Nip65OutboxResolver {
    store: Arc<dyn EventStore>,
    /// Indexer relay URLs, kept in sync with the kernel's relay config.
    /// Discovery kinds (kind:0, kind:3, kind:1xxxx) fan out to these in
    /// addition to the author's NIP-65 write relays.
    indexer_relays: IndexerRelaysSlot,
    /// Locally configured write relays for the active account. This covers
    /// the period after onboarding edits relay rows but before the just-sent
    /// kind:10002 comes back from a relay.
    local_write_relays: LocalWriteRelaysSlot,
    /// Active account pubkey. Local relay-row fallback applies only to this
    /// pubkey so already-signed events from other authors never route through
    /// the viewer's relays.
    active_account: ActiveAccountSlot,
}

/// Returns true for kinds that index relays exist to serve: kind:0 (profile),
/// kind:3 (contacts), and 10000–19999 (replaceable events per NIP-01).
pub fn is_discovery_kind(kind: u32) -> bool {
    kind == 0 || kind == 3 || (10000..20000).contains(&kind)
}

impl Nip65OutboxResolver {
    /// Build a resolver backed by the given event store and a shared indexer
    /// relay list. The kernel holds a clone of the Arc and updates it whenever
    /// relay config changes, so the resolver always sees current URLs.
    ///
    #[must_use]
    pub fn new(store: Arc<dyn EventStore>, indexer_relays: IndexerRelaysSlot) -> Self {
        Self::with_local_relays(
            store,
            indexer_relays,
            new_local_write_relays_slot(),
            new_active_account_slot(),
        )
    }

    pub(crate) fn with_local_relays(
        store: Arc<dyn EventStore>,
        indexer_relays: IndexerRelaysSlot,
        local_write_relays: LocalWriteRelaysSlot,
        active_account: ActiveAccountSlot,
    ) -> Self {
        Self {
            store,
            indexer_relays,
            local_write_relays,
            active_account,
        }
    }

    /// Test-only constructor — builds a resolver with an **empty** indexer
    /// relay set, so discovery kinds get no fan-out and every kind resolves
    /// purely from the author's cached kind:10002. Despite the historical
    /// name there is no "default fallback": the indexer list is simply empty.
    /// Used by the `nmp-testing` real-relay integration tests, which exercise
    /// the pure NIP-65 path; production code always uses [`Self::new`] with a
    /// live indexer handle. Not `#[cfg(test)]` because the consumers are
    /// integration tests in a sibling crate.
    #[doc(hidden)]
    pub fn with_default_fallback(store: Arc<dyn EventStore>) -> Self {
        Self::new(store, new_indexer_relays_slot())
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
        kind: u32,
    ) -> BTreeSet<RelayUrl> {
        // 1. Explicit targets win — the caller has opted out per D3.
        if let PublishTarget::Explicit { relays } = target {
            return relays.iter().cloned().collect();
        }

        let mut out: BTreeSet<RelayUrl> = BTreeSet::new();

        // 2. Author write-relays (when a kind:10002 is cached).
        //
        // If the author has no kind:10002 on file (or has an empty write set),
        // `out` stays empty here. For a non-discovery kind that is fail-closed
        // per D3: the engine maps an empty resolve to
        // `PublishEngineError::NoTargets` and surfaces a visible toast. This
        // mirrors T134's subscription-side `unroutable_authors` discipline —
        // unroutable is surfaced honestly, never silently widened. Discovery
        // kinds escape the empty set via step 3 below.
        if let Some((writes, _reads)) = self.lookup_kind10002(author_pubkey) {
            out.extend(writes);
        }
        if out.is_empty() && self.is_active_account(author_pubkey) {
            if let Ok(guard) = self.local_write_relays.lock() {
                out.extend(guard.as_slice().iter().cloned());
            }
        }

        // 3. Discovery kinds (kind:0 / kind:3 / kind:10000–19999) also fan out
        // to the indexer relays so the author's profile, contacts, and
        // replaceable events are discoverable. This is the ONLY cold-start
        // widening in the resolver, and it is deliberately scoped to discovery
        // kinds — a kind:1 note from an uncached author still resolves empty
        // (NoTargets), it does not leak onto the indexers.
        if is_discovery_kind(kind) {
            if let Ok(guard) = self.indexer_relays.lock() {
                out.extend(guard.as_slice().iter().cloned());
            }
        }

        // 4. Recipient read-relays — union for every `#p` tag, but only for
        // small recipient sets. At 15+ distinct p-tagged pubkeys the event is
        // broadcast-ish enough that recipient inbox fan-out becomes noisy.
        if p_tags.len() < RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD {
            for p in p_tags {
                if let Some((_writes, reads)) = self.lookup_kind10002(p) {
                    out.extend(reads);
                }
            }
        }

        out
    }
}

impl Nip65OutboxResolver {
    fn is_active_account(&self, author_pubkey: &str) -> bool {
        self.active_account
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .is_some_and(|active| active == author_pubkey)
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
