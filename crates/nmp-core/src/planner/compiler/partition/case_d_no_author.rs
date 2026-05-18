//! Case D: no authors, addresses, or `#p` → active-account read relays.
//!
//! Used for hashtag firehose queries and global search — interests that are
//! not scoped to any specific author or recipient. Routes to the active
//! account's read relays when available; falls through to the configured
//! indexer set when the read relays are empty.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestShape, LogicalInterest, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::RelayEntry;

/// Route a no-author/no-address/no-p interest to active-account or indexer relays.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    active_account_read_relays: &[RelayUrl],
    indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
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
