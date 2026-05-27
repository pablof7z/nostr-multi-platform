//! Case E: `relay_pin` hard-routing-pin → host relay only.
//!
//! This is the THIRD routing lane: when an interest declares a `relay_pin`
//! host relay, ALL four-lane logic (Cases A through D) is suppressed and the
//! interest goes to exactly the pinned URL. NIP-65 mailbox routing for any
//! author / address / `#p` value on the same interest is ignored — the pin
//! wins, by design.
//!
//! Why a separate case and not a mode flag on the other cases:
//! - The four cases each consult `MailboxCache` and may issue `request_probe`
//!   for cache misses; a pinned interest must NOT do either (it would leak
//!   the user's interest in the pinned pubkey to NIP-65 lookup paths it
//!   shouldn't touch).
//! - The pin is structural: the relay set is `{relay_pin.clone()}` regardless
//!   of authors/addresses/p — no lookup, no fallback, no indexer.
//! - Routing source is always `UserConfigured(Debug)` for the diagnostics
//!   lane — the pin is an operator-injected override that bypasses the
//!   normal four-lane discipline by design. Sub-categorising it as `Debug`
//!   keeps the four-lane four-lane: lane 4 absorbs all non-discoverable
//!   routing reasons, of which the pin is one.
//!
//! Doctrine: D3 (outbox routing automatic) — the pin is an explicit opt-out
//!           a protocol crate chooses by setting `relay_pin`, NOT a hand-roll
//!           bypass at the call site.
//!
//! Example use case: NIP-29 relay-based groups (the group exists on a single
//! host relay; cross-host routing would be incorrect).

use std::collections::{BTreeMap, BTreeSet};

use super::RelayEntry;
use crate::{
    interest::{InterestShape, LogicalInterest, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};

/// Route a pinned interest to its declared host relay only.
///
/// `pin_url` is `interest.shape.relay_pin.as_ref().unwrap().clone()` — the
/// caller guarantees `Some(_)` before invoking this routine.
///
/// `base_shape` carries the non-routing-derived fields (kinds, tags, since,
/// until, limit, `event_ids`); the partition retains `addresses` and `authors`
/// on the resulting entry so the wire-emitter sends the full filter shape.
/// Routing itself ignores them — only the pin matters.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    pin_url: &RelayUrl,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // Retain authors and addresses on the wire filter — relays still expect
    // them — but routing has already been decided as `pin_url` only.
    let authors_for_relay = interest.shape.authors.clone();
    let addresses_for_relay = interest.shape.addresses.clone();

    // Preserve the pin on base_shape so downstream merge (Rule 9) sees it.
    let mut base_with_pin = base_shape.clone();
    base_with_pin.relay_pin = Some(pin_url.clone());

    relay_entries
        .entry(pin_url.clone())
        .or_default()
        .push(RelayEntry {
            base_shape: base_with_pin,
            authors_for_relay,
            addresses_for_relay,
            lifecycle: interest.lifecycle.clone(),
            sources: BTreeSet::from([RoutingSource::UserConfigured(UserConfiguredCategory::Debug)]),
            interest_id: interest.id.clone(),
        });
}
