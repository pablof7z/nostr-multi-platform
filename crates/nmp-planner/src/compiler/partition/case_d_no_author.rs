//! Case D: no authors, addresses, or `#p` → active-account read relays ∪
//! app relays.
//!
//! Used for hashtag firehose queries and global search — interests that are
//! not scoped to any specific author or recipient. Per the routing-rules
//! clarification:
//!
//! - The hashtag firehose REQ goes to the UNION of the active account's
//!   `read_relays` and the kernel-configured `app_relays`. Both lanes
//!   (`UserConfigured(AccountRead)` and `UserConfigured(AppRelay)`) are
//!   recorded so diagnostics show why each URL was selected.
//! - When BOTH sets are empty, we fall through to the indexer set as a
//!   last-resort cold-start landing pad. This is the only remaining content
//!   path that touches the indexer set and exists purely so kernel-driven
//!   bootstrap traffic still lands somewhere before the user has configured
//!   anything; it is not a substitute for `app_relays` in normal operation.
//!
//! ## PD-033-C planner extension
//!
//! The sibling `route_bootstrap_content` helper handles the kernel-driven
//! discovery-oneshot case for referenced event ids. Callers (the partition
//! dispatcher in `partition::mod`) gate on `OneShot + Global + event_ids` and
//! invoke this helper BEFORE the normal Case D body, so a discovery REQ for
//! known event-id batches lands on a content relay
//! (`bootstrap_content_relays`) rather than the indexer set. Non-discovery
//! Case D interests (`Tailing` firehose, `Account`-scoped reads, anything
//! without concrete `event_ids`) still flow through `route` unchanged.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2,
//!          `docs/retired/pd033c-routing-gaps.md` §4.3
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use super::RelayEntry;
use crate::{
    interest::{InterestShape, LogicalInterest, RelayUrl},
    plan::{
        HintOrigin, InterestAttribution, RelayAttribution, RoutingSource, UserConfiguredCategory,
    },
};

/// Route no-author hints. Stacks with the normal Case D sources.
pub(super) fn route_hints(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // Accumulate both routing sources and hint origins per relay URL.
    let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<RoutingSource>, BTreeSet<HintOrigin>)> =
        BTreeMap::new();
    for hint in &interest.hints {
        let Some((relay_url, source)) = super::hint_helper::route_for_hint(hint) else {
            continue;
        };
        let origin = super::hint_helper::hint_origin_for(hint);
        let entry = per_relay.entry(relay_url).or_default();
        entry.0.insert(source);
        entry.1.insert(origin);
    }
    for (relay_url, (sources, hint_origins)) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
                attribution: RelayAttribution {
                    hints: hint_origins,
                    interests: vec![InterestAttribution {
                        interest_id: interest.id.clone(),
                        kinds: interest.shape.kinds.clone(),
                        authors: BTreeSet::new(),
                    }],
                    ..RelayAttribution::default()
                },
            });
    }
}

/// Route a no-author/no-address/no-p interest to active-account ∪ `app_relays`.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    active_account_read_relays: &[RelayUrl],
    app_relays: &[RelayUrl],
    indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // Per-URL source accumulator so a relay that appears in BOTH
    // `active_account_read_relays` and `app_relays` records both lanes
    // (`AccountRead` ∪ `AppRelay`) rather than collapsing to whichever set
    // was iterated last.
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();

    for relay in active_account_read_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::AccountRead,
            ));
    }

    for relay in app_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::AppRelay,
            ));
    }

    // Cold-start indexer fallback: ONLY when both user-configured sources
    // produced zero URLs do we fall through to the indexer. This preserves
    // bootstrap behaviour for kernel-driven discovery REQs (kind:0/3/10002)
    // that legitimately fire before any account configuration is loaded.
    if per_relay.is_empty() {
        for relay in indexer_relays {
            per_relay
                .entry(relay.clone())
                .or_default()
                .insert(RoutingSource::UserConfigured(
                    UserConfiguredCategory::Indexer,
                ));
        }
    }

    for (relay_url, sources) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
                attribution: RelayAttribution::default(),
            });
    }
}

/// PD-033-C planner extension: route a `OneShot + Global + event_ids` discovery
/// interest to `bootstrap_content_relays`.
///
/// All emitted entries are tagged
/// `RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap)` — a
/// distinct lane sub-category so diagnostics can tell "cold-start discovery
/// fetch landed here" apart from "user-configured app relay carried this
/// content" (`AppRelay`) or "indexer carried this fallback firehose"
/// (`Indexer`).
pub(super) fn route_bootstrap_content(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    bootstrap_content_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();
    for relay in bootstrap_content_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::Bootstrap,
            ));
    }
    for (relay_url, sources) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
                attribution: RelayAttribution::default(),
            });
    }
}

#[cfg(test)]
mod tests;
