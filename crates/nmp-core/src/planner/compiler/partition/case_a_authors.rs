//! Case A: explicit `authors` → Outbox (write relays).
//!
//! When an interest carries both `authors` AND `#p` tag values, this case
//! emits Outbox entries AND calls `inbox_helper::route_p_tags_to_inbox` for
//! the "both populated" split (spec §3.1 "Both populated" row).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::{MailboxCache, RelayEntry};
use super::inbox_helper::route_p_tags_to_inbox;

/// Per-relay accumulator: (authors, addresses, sources).
type CaseAEntry = (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>);

/// Route an interest with explicit authors to outbox relays.
///
/// Also emits inbox entries for any `#p` tag values ("both populated" split).
/// The inbox slice carries the original `authors` so the REQ semantics remain
/// `authors AND #p` (intersection) rather than a wildcard over all #p events.
pub(super) fn route(
    interest: &LogicalInterest,
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // Accumulate per-relay (authors, addresses, sources) before pushing
    // RelayEntry objects. This lets multiple authors share a relay without
    // creating separate entries — Stage 3 merge operates on the combined set.
    //
    // The per_relay map accumulates ALL RoutingSource lanes per relay URL.
    // Author A may reach wss://x via NIP-65 while author B reaches the same
    // relay via indexer fallback; both lanes must be recorded so Stage 3
    // role_tags reflects the four-lane model (§3.1 four-lane discipline).
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
            p_tag_values,
            &interest.shape.authors,
            base_shape,
            &interest.lifecycle,
            &interest.id,
            mailbox_cache,
            relay_entries,
        );
    }
}
