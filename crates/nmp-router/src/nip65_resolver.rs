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
//!
//! **Crate-boundary spec §271 (2026-05-25)**: this resolver lives in
//! `nmp-router`, not `nmp-core`. The publish-side `OutboxResolver` trait
//! stays in `nmp_core::publish::traits` so the kernel can carry an `Arc<dyn
//! OutboxResolver>` without naming a NIP. Production composition
//! (`nmp-defaults::register_defaults`) wires a `Nip65OutboxResolver`
//! into the kernel via `AppHost::set_publish_resolver_factory` →
//! `Kernel::set_publish_resolver`. This keeps `nmp-core` protocol-neutral
//! (D0) and reflects the fact that NIP-65 outbox resolution is the same
//! algorithmic concern as the substrate `OutboxRouter`
//! (`GenericOutboxRouter`, same crate).

use std::sync::Arc;

use nmp_core::kinds::KIND_RELAY_LIST;
use nmp_core::publish::{
    OutboxResolver, PublishTarget, RelaySelectionReason, RelayUrl, ResolvedRelay,
};
use nmp_core::slots::{
    new_active_account_slot, new_indexer_relays_slot, new_local_write_relays_slot,
    ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::substrate::{canonicalize_relay_url, BlockedRelaySet};
use nmp_kinds::ptags_are_recipients;
use nmp_store::{EventStore, PubKey, StoredEvent};

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

// Canonical definition lives in `discovery`; re-exported here so `lib.rs` and
// external callers keep the same public path (`nmp_router::is_discovery_kind`).
pub use crate::discovery::is_discovery_kind;

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

    /// Build a resolver with explicit handles for every slot the kernel
    /// shares with it. Production composition uses this constructor so the
    /// kernel and the resolver read the same `IndexerRelaysSlot` /
    /// `LocalWriteRelaysSlot` / `ActiveAccountSlot` instances — the actor is
    /// the sole writer, the resolver is a reader, D4 holds.
    #[must_use]
    pub fn with_local_relays(
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
    #[must_use]
    pub fn with_default_fallback(store: Arc<dyn EventStore>) -> Self {
        Self::new(store, new_indexer_relays_slot())
    }

    /// Look up the latest kind:10002 for `author_hex` and parse it into
    /// `(write_relays, read_relays)`. `(both, both)` is the unmarked case.
    fn lookup_kind10002(&self, author_hex: &str) -> Option<(Vec<RelayUrl>, Vec<RelayUrl>)> {
        let author = hex_to_pubkey(author_hex)?;
        let iter = self
            .store
            .scan_by_author_kind(&author, &[KIND_RELAY_LIST], None, None, 1)
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
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        // 1. Explicit targets win — the caller has opted out per D3 — but a
        //    blocked relay is blocked even when explicitly named (blocking is
        //    a privacy decision the resolver honours unconditionally).
        if let PublishTarget::Explicit {
            relays,
            route_class,
        } = target
        {
            return relays
                .iter()
                .filter(|url| !blocked.contains(url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit {
                        route_class: *route_class,
                    },
                })
                .collect();
        }

        let mut out: Vec<ResolvedRelay> = Vec::new();

        // 2. Author write-relays (when a kind:10002 is cached).
        //
        // Two distinct cases:
        //
        // a) `lookup_kind10002` returns `None` — the author has no kind:10002
        //    on file at all. This is the cold-start / bootstrap window: the
        //    active account has onboarded but the just-published kind:10002 has
        //    not yet come back from a relay. In this case the local_write_relays
        //    fallback (step 2b) is consulted so the user can still publish.
        //
        // b) `lookup_kind10002` returns `Some((writes, _))` — a kind:10002
        //    *exists*, even if `writes` is empty (all entries are read-marked).
        //    An empty write set is a deliberate "publish nowhere" signal; the
        //    local_write_relays fallback must NOT override it. For a
        //    non-discovery kind this is fail-closed per D3: the engine maps the
        //    empty resolve to `PublishEngineError::NoTargets` and surfaces a
        //    visible toast. This mirrors T134's subscription-side
        //    `unroutable_authors` discipline — unroutable is surfaced honestly,
        //    never silently widened. Discovery kinds escape via step 3 below.
        let kind10002 = self.lookup_kind10002(author_pubkey);
        if let Some((writes, _reads)) = &kind10002 {
            for url in writes.iter().cloned() {
                out.push(ResolvedRelay {
                    url,
                    reason: RelaySelectionReason::AuthorWriteRelay,
                });
            }
        }
        // Bootstrap fallback: only when no kind:10002 exists at all (None).
        // A Some with an empty write list is a deliberate "publish nowhere"
        // — do not override it with locally configured relays.
        if kind10002.is_none() && self.is_active_account(author_pubkey) {
            if let Ok(guard) = self.local_write_relays.lock() {
                for url in guard.as_slice().iter().cloned() {
                    out.push(ResolvedRelay {
                        url,
                        reason: RelaySelectionReason::LocalConfigRelay,
                    });
                }
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
                for url in guard.as_slice().iter().cloned() {
                    out.push(ResolvedRelay {
                        url,
                        reason: RelaySelectionReason::DiscoveryIndexer { kind },
                    });
                }
            }
        }

        // 4. Recipient read-relays — union for every `#p` tag, but ONLY when the
        // kind's `#p` tags semantically denote message recipients (people to notify),
        // AND only for small recipient sets. At 15+ distinct p-tagged pubkeys the
        // event is broadcast-ish enough that recipient inbox fan-out becomes noisy.
        //
        // Replaceable and addressable events (kind:0, kind:3, kind:10000–19999,
        // kind:30000–39999) use `#p` tags to list follows, mutes, or list members
        // (SUBJECTS), NOT to address a message to those pubkeys. Routing a kind:3
        // contact list to every followee's inbox relay is incorrect; those pubkeys
        // are subjects of the list, not intended receivers of the publish. The
        // `ptags_are_recipients` predicate captures this semantic distinction
        // without a hardcoded kind allowlist, derived purely from the NIP-01
        // replaceable / addressable classification.
        if ptags_are_recipients(kind) && p_tags.len() < RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD {
            for p in p_tags {
                if let Some((_writes, reads)) = self.lookup_kind10002(p) {
                    for url in reads {
                        out.push(ResolvedRelay {
                            url,
                            reason: RelaySelectionReason::RecipientInbox { pubkey: p.clone() },
                        });
                    }
                }
            }
        }

        // 5. Blocked-relay post-filter (kind:10006). The author told us to
        //    never publish to these relays; honour it across EVERY lane above
        //    (author write set, local-config fallback, discovery indexers,
        //    recipient inboxes). Without this an author's events leaked to a
        //    relay they explicitly blocked — the subscribe-side router has
        //    always filtered blocked relays per-lane; the publish-side
        //    resolver must too. Canonicalisation parity (kind:10002 ingest now
        //    canonicalises URLs, matching the blocked cache's canonical keys)
        //    makes this `contains` check match across host-case differences.
        out.retain(|r| !blocked.contains(&r.url));

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
        let Some(raw_url) = tag.get(1) else {
            tracing::debug!(target: "nmp.router.nip65", reason = "missing url", "skipping malformed kind:10002 tag");
            continue;
        };
        // Canonicalize via the single workspace authority (nmp-relay-url Layer 0).
        // Fail-closed: a tag that cannot be canonicalized (bad scheme, missing
        // authority, etc.) is dropped so it can never bypass the blocked-relay
        // filter under a different spelling.
        let Some(url) = canonicalize_relay_url(raw_url) else {
            tracing::debug!(target: "nmp.router.nip65", url = %raw_url, reason = "un-canonicalizable url", "skipping malformed kind:10002 tag");
            continue;
        };
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

#[cfg(test)]
#[path = "nip65_resolver/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "nip65_resolver/blocked_tests.rs"]
mod blocked_tests;

#[cfg(test)]
#[path = "nip65_resolver/ptags_recipient_tests.rs"]
mod ptags_recipient_tests;
