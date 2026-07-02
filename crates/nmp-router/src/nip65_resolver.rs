//! `Nip65OutboxResolver` — concrete `OutboxResolver` impl reading parsed
//! kind:10002 relay-list facts from a `MailboxCache`.
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
//! D7 (capabilities report): bad-shape kind:10002 tags are handled by
//! [`Kind10002Parser`](crate::Kind10002Parser). The resolver consumes only the
//! parsed cache shape and never re-parses raw event tags.
//!
//! **Crate-boundary spec §271 (2026-05-25)**: this resolver lives in
//! `nmp-router`, not `nmp-core`. The publish-side `OutboxResolver` trait
//! stays in `nmp_core::publish::traits` so the kernel can carry an `Arc<dyn
//! OutboxResolver>` without naming a NIP. Production composition wires a
//! `Nip65OutboxResolver` into the kernel via
//! `AppHost::set_publish_resolver_factory` →
//! `Kernel::set_publish_resolver`. This keeps `nmp-core` protocol-neutral
//! (D0) and reflects the fact that NIP-65 outbox resolution is the same
//! algorithmic concern as the substrate `OutboxRouter`
//! (`GenericOutboxRouter`, same crate).

use std::sync::Arc;

use nmp_core::publish::{
    OutboxResolver, PublishTarget, RelaySelectionReason, RelayUrl, ResolvedRelay,
};
use nmp_core::slots::{
    new_active_account_slot, new_indexer_relays_slot, new_local_write_relays_slot,
    ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::substrate::{BlockedRelaySet, MailboxCache};
use nmp_kinds::ptags_are_recipients;

/// Maximum distinct `#p` pubkeys that still get recipient inbox fan-out.
///
/// Events with this many or more tagged pubkeys are treated as broadcast-ish:
/// publish to the author's own write relays, and for discovery kinds to
/// indexers, but do not fan out to every tagged pubkey's read relays.
pub const RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD: usize = 15;

/// Resolve `PublishTarget::Auto` to a concrete relay set per NIP-65, using a
/// `MailboxCache` as the source of truth for parsed kind:10002 lookups.
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
    mailbox_cache: Arc<dyn MailboxCache>,
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
    /// Build a resolver backed by the given mailbox cache and a shared indexer
    /// relay list. The cache must be the same instance written by
    /// [`Kind10002Parser`](crate::Kind10002Parser).
    ///
    #[must_use]
    pub fn new(mailbox_cache: Arc<dyn MailboxCache>, indexer_relays: IndexerRelaysSlot) -> Self {
        Self::with_local_relays(
            mailbox_cache,
            indexer_relays,
            new_local_write_relays_slot(),
            new_active_account_slot(),
        )
    }

    /// Build a resolver with explicit handles for every slot the kernel
    /// shares with it. Production composition uses this constructor so the
    /// kernel and the resolver read the same `MailboxCache` /
    /// `IndexerRelaysSlot` /
    /// `LocalWriteRelaysSlot` / `ActiveAccountSlot` instances — the actor is
    /// the sole writer, the resolver is a reader, D4 holds.
    #[must_use]
    pub fn with_local_relays(
        mailbox_cache: Arc<dyn MailboxCache>,
        indexer_relays: IndexerRelaysSlot,
        local_write_relays: LocalWriteRelaysSlot,
        active_account: ActiveAccountSlot,
    ) -> Self {
        Self {
            mailbox_cache,
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
    pub fn with_default_fallback(mailbox_cache: Arc<dyn MailboxCache>) -> Self {
        Self::new(mailbox_cache, new_indexer_relays_slot())
    }

    /// Look up the parsed kind:10002 facts for `author_hex` and return
    /// `(write_relays, read_relays)`. `(both, both)` is the unmarked case.
    fn lookup_kind10002(&self, author_hex: &str) -> Option<(Vec<RelayUrl>, Vec<RelayUrl>)> {
        let parsed = self.mailbox_cache.snapshot(&author_hex.to_string())?;
        Some((parsed.write_set(), parsed.read_set()))
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

        // 5. Blocked-relay post-filter. The author told us to
        //    never publish to these relays; honour it across EVERY lane above
        //    (author write set, local-config fallback, discovery indexers,
        //    recipient inboxes). Without this an author's events leaked to a
        //    relay they explicitly blocked — the subscribe-side router has
        //    always filtered blocked relays per-lane; the publish-side
        //    resolver must too. Canonicalisation parity makes this `contains`
        //    check match across host-case differences.
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

#[cfg(test)]
#[path = "nip65_resolver/tests/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "nip65_resolver/blocked_tests.rs"]
mod blocked_tests;

#[cfg(test)]
#[path = "nip65_resolver/ptags_recipient_tests.rs"]
mod ptags_recipient_tests;
