//! `RelayEntry` and `partition_interest`: Stage 1+2 of the compiler pipeline.
//!
//! Partitions a single `LogicalInterest` into per-relay entries, with each
//! entry carrying only the authors that declared the relay (author-partitioning).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{
        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
        RelayUrl,
    },
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::mailbox::MailboxCache;

// ─── RelayEntry ──────────────────────────────────────────────────────────────

/// A relay-partitioned slice of one logical interest.
///
/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
/// authors that declared this specific relay (not all N authors). This is the
/// author-partitioning that lets the merge lattice produce per-relay author
/// subsets.
///
/// `sources` is a set (not a single value) so that a relay reached by two
/// different lanes (e.g. NIP-65 for author A, Indexer for author B) preserves
/// both lanes in `role_tags` at Stage 3 (§3.1 four-lane discipline).
pub(super) struct RelayEntry {
    /// The interest's non-author fields (kinds, tags, since, until, etc.).
    /// `authors` is intentionally left empty here; we merge `authors_for_relay`
    /// in at Stage 3 merge time.
    pub base_shape: InterestShape,
    /// The subset of authors from this interest that declared this relay.
    pub authors_for_relay: BTreeSet<Pubkey>,
    /// Address-pointer coordinates from this interest (if relevant for routing).
    pub addresses_for_relay: BTreeSet<NaddrCoord>,
    pub lifecycle: InterestLifecycle,
    /// All routing lanes that contributed to this relay entry.
    pub sources: BTreeSet<RoutingSource>,
    pub interest_id: InterestId,
}

impl RelayEntry {
    /// Construct the final `InterestShape` for this relay slice.
    pub fn into_shape(
        mut self,
    ) -> (InterestShape, InterestLifecycle, BTreeSet<RoutingSource>, InterestId) {
        self.base_shape.authors = self.authors_for_relay;
        self.base_shape.addresses = self.addresses_for_relay;
        (self.base_shape, self.lifecycle, self.sources, self.interest_id)
    }
}

// ─── Internal type aliases ────────────────────────────────────────────────────

/// Per-relay accumulator for Case A: (authors, addresses, sources).
type CaseAEntry = (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>);

// ─── partition_interest ───────────────────────────────────────────────────────

/// Stage 1 + 2: partition one logical interest into per-relay entries.
///
/// Each entry carries only the AUTHORS that declared the specific relay,
/// preserving per-relay author-subset semantics (Assertion 2, §3.3).
///
/// ## Direction routing (§3.1 / §3.2)
///
/// - **Case A**: explicit `authors` → Outbox (write relays). Also routes
///   any `addresses` on the same interest to the same relay map. If the
///   interest also has `#p` tag values, inbox routing is emitted in addition
///   (see `route_p_tags_to_inbox`; spec §3.1 "Both populated" row).
/// - **Case B**: no authors, but `addresses` → Outbox for each coord.pubkey.
/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
///   (tagged pubkey's read relays). Structural ban enforced: never route
///   `#p` interests to non-inbox relays. When inbox relays are unknown, the
///   interest produces NO relay entries (fail-closed); a probe is emitted so
///   the next recompile has data.
/// - **Case D (no-author)**: no authors, addresses, or #p → active-account
///   read relays (hashtag firehose, global search). Falls to indexer if empty.
pub(super) fn partition_interest(
    interest: &LogicalInterest,
    mailbox_cache: &dyn MailboxCache,
    indexer_relays: &[RelayUrl],
    active_account_read_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let base_shape = InterestShape {
        authors: BTreeSet::new(),
        kinds: interest.shape.kinds.clone(),
        tags: interest.shape.tags.clone(),
        since: interest.shape.since,
        until: interest.shape.until,
        limit: interest.shape.limit,
        event_ids: interest.shape.event_ids.clone(),
        addresses: BTreeSet::new(),
    };

    // Extract #p tag values once — used in Case A (if authors + #p) and Case C.
    let p_tag_values: BTreeSet<Pubkey> = interest
        .shape
        .tags
        .get("p")
        .cloned()
        .unwrap_or_default();

    // Case A: explicit authors → Outbox (write relays).
    //
    // The per_relay map accumulates ALL RoutingSource lanes per relay URL.
    // Author A may reach wss://x via NIP-65 while author B reaches the same
    // relay via indexer fallback; both lanes must be recorded so Stage 3
    // role_tags reflects the four-lane model (§3.1 four-lane discipline).
    //
    // Per spec §3.1 "Both populated" row: if this interest also carries `#p`
    // tag values, we additionally emit Inbox entries for those tagged pubkeys
    // (via `route_p_tags_to_inbox`) after the Outbox entries are committed.
    if !interest.shape.authors.is_empty() {
        let mut per_relay: BTreeMap<RelayUrl, CaseAEntry> = BTreeMap::new();

        for author in &interest.shape.authors {
            match mailbox_cache.get(author) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                        entry.0.insert(author.clone());
                        entry.2.insert(RoutingSource::Nip65);
                    }
                }
                None => {
                    // No mailbox known — probe so cache can be populated and
                    // the next recompile routes via NIP-65 (§3.2).
                    mailbox_cache.request_probe(author);
                    for relay in indexer_relays {
                        let entry = per_relay
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                        entry.0.insert(author.clone());
                        entry.2.insert(RoutingSource::UserConfigured(
                            UserConfiguredCategory::Indexer,
                        ));
                    }
                }
            }
        }

        for coord in &interest.shape.addresses {
            match mailbox_cache.get(&coord.pubkey) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                        entry.1.insert(coord.clone());
                        entry.2.insert(RoutingSource::Nip65);
                    }
                }
                None => {
                    mailbox_cache.request_probe(&coord.pubkey);
                    for relay in indexer_relays {
                        let entry = per_relay
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                        entry.1.insert(coord.clone());
                        entry.2.insert(RoutingSource::UserConfigured(
                            UserConfiguredCategory::Indexer,
                        ));
                    }
                }
            }
        }

        for (relay_url, (authors, addrs, sources)) in per_relay {
            relay_entries.entry(relay_url).or_default().push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: authors,
                addresses_for_relay: addrs,
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
            });
        }

        // "Both populated" split: also emit Inbox entries for any #p values.
        //
        // The inbox slice preserves the original author constraint — the
        // semantics are `authors AND #p` (intersection), so the inbox REQ on
        // the tagged pubkey's read relays must still be filtered by the
        // interest's authors, not a wildcard that would match every event
        // tagging the recipient.
        if !p_tag_values.is_empty() {
            route_p_tags_to_inbox(
                &p_tag_values,
                &interest.shape.authors,
                &base_shape,
                &interest.lifecycle,
                &interest.id,
                mailbox_cache,
                relay_entries,
            );
        }
        return;
    }

    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
    if !interest.shape.addresses.is_empty() {
        let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>)> =
            BTreeMap::new();

        for coord in &interest.shape.addresses {
            match mailbox_cache.get(&coord.pubkey) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay_addrs
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
                        entry.0.insert(coord.clone());
                        entry.1.insert(RoutingSource::Nip65);
                    }
                }
                None => {
                    // Probe so the cache can route via NIP-65 on next recompile.
                    mailbox_cache.request_probe(&coord.pubkey);
                    for relay in indexer_relays {
                        let entry = per_relay_addrs
                            .entry(relay.clone())
                            .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
                        entry.0.insert(coord.clone());
                        entry.1.insert(RoutingSource::UserConfigured(
                            UserConfiguredCategory::Indexer,
                        ));
                    }
                }
            }
        }

        for (relay_url, (addrs, sources)) in per_relay_addrs {
            relay_entries.entry(relay_url).or_default().push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: addrs,
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
            });
        }
        return;
    }

    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
    //
    // Structural ban: `#p` interests MUST route to inbox relays only. We never
    // route to the author's write relays, and we do not fall back to the indexer
    // set — that would route DM-relevant queries through a public relay without
    // the recipient's explicit read-relay declaration (§3.1 / §3.2).
    //
    // When inbox relays are unknown, we emit NO relay entries (fail-closed) and
    // emit a probe so the next recompile has data. The plan will have an empty
    // per_relay map for this interest until kind:10002 arrives.
    if !p_tag_values.is_empty() {
        // Case C: no `authors` (the early return above ensures this) — pass an
        // empty set so the inbox shape doesn't constrain authors.
        let empty_authors: BTreeSet<Pubkey> = BTreeSet::new();
        route_p_tags_to_inbox(
            &p_tag_values,
            &empty_authors,
            &base_shape,
            &interest.lifecycle,
            &interest.id,
            mailbox_cache,
            relay_entries,
        );
        return;
    }

    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
    let (fallback_relays, fallback_source) = if !active_account_read_relays.is_empty() {
        (
            active_account_read_relays,
            RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead),
        )
    } else {
        (
            indexer_relays,
            RoutingSource::UserConfigured(UserConfiguredCategory::Indexer),
        )
    };
    for relay in fallback_relays {
        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: BTreeSet::new(),
            lifecycle: interest.lifecycle.clone(),
            sources: BTreeSet::from([fallback_source.clone()]),
            interest_id: interest.id.clone(),
        });
    }
}

// ─── Inbox routing helper ─────────────────────────────────────────────────────

/// Route `#p` tag values to their inbox relays (read ∪ both).
///
/// `authors_for_inbox` is the original interest's author set — when called
/// from Case A's "both populated" split, this preserves the `authors AND #p`
/// semantics on the inbox slice (the inbox REQ filters by Alice's authorship
/// AND tags Bob, not "any event tagging Bob"). Case C passes an empty set.
///
/// Structural ban: if inbox relays are unknown for a tagged pubkey, emit
/// NO relay entries for that pubkey (fail-closed). A probe is emitted so
/// the next recompile has data (§3.2 — IndexerProbe side-effect).
///
/// Stage 3 (mod.rs) is responsible for deduping repeated `interest_id`
/// pushes on the same relay (e.g. when an outbox and inbox push land on the
/// same relay URL because the author's write relay == a tagged pubkey's
/// read relay).
///
/// Called from Case A ("both populated" split) and Case C.
fn route_p_tags_to_inbox(
    p_tag_values: &BTreeSet<Pubkey>,
    authors_for_inbox: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    lifecycle: &InterestLifecycle,
    interest_id: &InterestId,
    mailbox_cache: &dyn MailboxCache,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    for tagged_pk in p_tag_values {
        match mailbox_cache.get(tagged_pk) {
            Some(ref snapshot) if snapshot.has_inbox_relays() => {
                for relay in snapshot.inbox_relays() {
                    relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
                        base_shape: base_shape.clone(),
                        authors_for_relay: authors_for_inbox.clone(),
                        addresses_for_relay: BTreeSet::new(),
                        lifecycle: lifecycle.clone(),
                        sources: BTreeSet::from([RoutingSource::Nip65]),
                        interest_id: interest_id.clone(),
                    });
                }
            }
            // Inbox unknown or empty: fail-closed. Probe and emit nothing.
            _ => {
                mailbox_cache.request_probe(tagged_pk);
            }
        }
    }
}
