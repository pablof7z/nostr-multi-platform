//! Case B: no explicit authors, but `addresses` → Outbox (write relays).
//!
//! Routes address-pointer coordinates to the coordinate pubkey's outbox
//! relays. Falls back to the indexer when the pubkey has no known mailbox.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestShape, LogicalInterest, NaddrCoord, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::{MailboxCache, RelayEntry};

/// Route an interest with address-pointer pubkeys to their outbox relays.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>)> =
        BTreeMap::new();

    for coord in &interest.shape.addresses {
        match mailbox_cache.get(&coord.pubkey) {
            Some(snapshot) => {
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
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
                    let entry = per_relay
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

    for (relay_url, (addrs, sources)) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: addrs,
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }
}
