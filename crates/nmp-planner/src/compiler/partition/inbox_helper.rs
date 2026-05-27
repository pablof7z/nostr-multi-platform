//! `route_p_tags_to_inbox`: shared inbox routing helper for Cases A and C.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic), structural ban on indexer fallback.

use std::collections::{BTreeMap, BTreeSet};

use super::{MailboxCache, RelayEntry};
use crate::{
    interest::{InterestId, InterestLifecycle, InterestShape, PTagRouting, Pubkey, RelayUrl},
    plan::RoutingSource,
};

/// Route `#p` tag values to their inbox relays (read ∪ both).
///
/// `authors_for_inbox` is the original interest's author set — when called
/// from Case A's "both populated" split, this preserves the `authors AND #p`
/// semantics on the inbox slice (the inbox REQ filters by Alice's authorship
/// AND tags Bob, not "any event tagging Bob"). Case C passes an empty set.
///
/// Per-pubkey `#p` scoping: the inbox shape narrows `tags["p"]` to a
/// singleton `{tagged_pk}`. Without this, Bob's inbox relay would receive a
/// REQ with `#p=[Bob, Carol]` — leaking Carol's tag onto Bob's relay and
/// over-fetching events that should arrive via Carol's own inbox relay.
///
/// Structural ban: if inbox relays are unknown for a tagged pubkey, emit
/// NO relay entries for that pubkey (fail-closed). A probe is emitted so
/// the next recompile has data (§3.2 — `IndexerProbe` side-effect).
///
/// Stage 3 (mod.rs) is responsible for deduping repeated `interest_id`
/// pushes on the same relay (e.g. when an outbox and inbox push land on the
/// same relay URL because the author's write relay == a tagged pubkey's
/// read relay).
///
/// Called from Case A ("both populated" split) and Case C.
pub(super) fn route_p_tags_to_inbox(
    p_tag_values: &BTreeSet<Pubkey>,
    authors_for_inbox: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    lifecycle: &InterestLifecycle,
    interest_id: &InterestId,
    mailbox_cache: &dyn MailboxCache,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    for tagged_pk in p_tag_values {
        let relays_and_source = match base_shape.p_tag_routing {
            PTagRouting::Nip65ReadRelays => match mailbox_cache.get(tagged_pk) {
                Some(ref snapshot) if snapshot.has_inbox_relays() => Some((
                    snapshot.inbox_relays().cloned().collect(),
                    RoutingSource::Nip65,
                )),
                // Inbox unknown or empty: fail-closed. Probe and emit nothing.
                _ => {
                    mailbox_cache.request_probe(tagged_pk);
                    None
                }
            },
            PTagRouting::Nip17DmRelays => mailbox_cache
                .dm_inbox_relays(tagged_pk)
                .filter(|relays| !relays.is_empty())
                .map(|relays| (relays, RoutingSource::Nip17DmRelay)),
        };

        if let Some((relays, source)) = relays_and_source {
            // Narrow `#p` to the singleton `{tagged_pk}` for this relay.
            let mut per_pk_shape = base_shape.clone();
            per_pk_shape.tags.insert(
                "p".to_string(),
                std::iter::once(tagged_pk.clone()).collect(),
            );
            for relay in relays {
                relay_entries.entry(relay).or_default().push(RelayEntry {
                    base_shape: per_pk_shape.clone(),
                    authors_for_relay: authors_for_inbox.clone(),
                    addresses_for_relay: BTreeSet::new(),
                    lifecycle: lifecycle.clone(),
                    sources: BTreeSet::from([source.clone()]),
                    interest_id: interest_id.clone(),
                });
            }
        }
    }
}
