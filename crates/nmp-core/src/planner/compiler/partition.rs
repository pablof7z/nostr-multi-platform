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
    pub source: RoutingSource,
    pub interest_id: InterestId,
}

impl RelayEntry {
    /// Construct the final `InterestShape` for this relay slice.
    pub fn into_shape(mut self) -> (InterestShape, InterestLifecycle, RoutingSource, InterestId) {
        self.base_shape.authors = self.authors_for_relay;
        self.base_shape.addresses = self.addresses_for_relay;
        (self.base_shape, self.lifecycle, self.source, self.interest_id)
    }
}

// ─── partition_interest ───────────────────────────────────────────────────────

/// Stage 1 + 2: partition one logical interest into per-relay entries.
///
/// Each entry carries only the AUTHORS that declared the specific relay,
/// preserving per-relay author-subset semantics (Assertion 2, §3.3).
///
/// ## Direction routing (§3.1 / §3.2)
///
/// - **Case A**: explicit `authors` → Outbox (write relays). Also routes
///   any `addresses` on the same interest to the same relay map.
/// - **Case B**: no authors, but `addresses` → Outbox for each coord.pubkey.
/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
///   (tagged pubkey's read relays). Structural ban enforced: never route
///   private `#p` interests to non-inbox relays.
///   Phase 1 stub: falls back to indexer; real inbox resolution in phase 2.
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

    // Case A: explicit authors → Outbox (write relays).
    if !interest.shape.authors.is_empty() {
        let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)> =
            BTreeMap::new();

        for author in &interest.shape.authors {
            match mailbox_cache.get(author) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
                        });
                        entry.0.insert(author.clone());
                    }
                }
                None => {
                    for relay in indexer_relays {
                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(), BTreeSet::new(),
                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
                        });
                        entry.0.insert(author.clone());
                    }
                }
            }
        }

        for coord in &interest.shape.addresses {
            match mailbox_cache.get(&coord.pubkey) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
                        });
                        entry.1.insert(coord.clone());
                    }
                }
                None => {
                    for relay in indexer_relays {
                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(), BTreeSet::new(),
                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
                        });
                        entry.1.insert(coord.clone());
                    }
                }
            }
        }

        for (relay_url, (authors, addrs, source)) in per_relay {
            relay_entries.entry(relay_url).or_default().push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: authors,
                addresses_for_relay: addrs,
                lifecycle: interest.lifecycle.clone(),
                source,
                interest_id: interest.id.clone(),
            });
        }
        return;
    }

    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
    if !interest.shape.addresses.is_empty() {
        let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, RoutingSource)> =
            BTreeMap::new();

        for coord in &interest.shape.addresses {
            match mailbox_cache.get(&coord.pubkey) {
                Some(snapshot) => {
                    for relay in snapshot.outbox_relays() {
                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(), RoutingSource::Nip65)
                        });
                        entry.0.insert(coord.clone());
                    }
                }
                None => {
                    for relay in indexer_relays {
                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
                            (BTreeSet::new(),
                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
                        });
                        entry.0.insert(coord.clone());
                    }
                }
            }
        }

        for (relay_url, (addrs, source)) in per_relay_addrs {
            relay_entries.entry(relay_url).or_default().push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: addrs,
                lifecycle: interest.lifecycle.clone(),
                source,
                interest_id: interest.id.clone(),
            });
        }
        return;
    }

    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
    //
    // #p interests (DMs, notifications) MUST route to the tagged pubkey's READ
    // relays (Inbox direction). Routing them to write relays violates the
    // structural ban on private routes to non-inbox relays (§3.2).
    //
    // Phase 1 stub: read_relays not yet populated from kind:10002 → fall back
    // to indexer. The code path is correct; only the mailbox data is missing.
    let p_tag_values: BTreeSet<Pubkey> = interest
        .shape
        .tags
        .get("p")
        .cloned()
        .unwrap_or_default();

    if !p_tag_values.is_empty() {
        for tagged_pk in &p_tag_values {
            match mailbox_cache.get(tagged_pk) {
                Some(snapshot) if !snapshot.read_relays.is_empty() => {
                    for relay in &snapshot.read_relays {
                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
                            base_shape: base_shape.clone(),
                            authors_for_relay: BTreeSet::new(),
                            addresses_for_relay: BTreeSet::new(),
                            lifecycle: interest.lifecycle.clone(),
                            source: RoutingSource::Nip65,
                            interest_id: interest.id.clone(),
                        });
                    }
                }
                _ => {
                    mailbox_cache.request_probe(tagged_pk);
                    for relay in indexer_relays {
                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
                            base_shape: base_shape.clone(),
                            authors_for_relay: BTreeSet::new(),
                            addresses_for_relay: BTreeSet::new(),
                            lifecycle: interest.lifecycle.clone(),
                            source: RoutingSource::UserConfigured(
                                UserConfiguredCategory::Indexer,
                            ),
                            interest_id: interest.id.clone(),
                        });
                    }
                }
            }
        }
        return;
    }

    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
    let (fallback_relays, fallback_source) = if !active_account_read_relays.is_empty() {
        (active_account_read_relays,
         RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead))
    } else {
        (indexer_relays,
         RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
    };
    for relay in fallback_relays {
        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: BTreeSet::new(),
            lifecycle: interest.lifecycle.clone(),
            source: fallback_source.clone(),
            interest_id: interest.id.clone(),
        });
    }
}
