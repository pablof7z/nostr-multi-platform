//! Case C: `#p` tag values → Inbox.
//!
//! Structural ban: `#p` interests MUST route to the inbox relay set selected
//! by the interest. Generic `#p` interests use NIP-65 read relays; NIP-17
//! gift-wrap inbox interests use kind:10050 DM relays. We never route to the
//! author's write relays, and we do not fall back to the indexer set.
//!
//! When inbox relays are unknown, we emit NO relay entries (fail-closed) and
//! emit a probe so the next recompile has data. The plan will have an empty
//! per_relay map for this interest until kind:10002 or kind:10050 arrives.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::interest::{InterestId, InterestLifecycle, InterestShape, Pubkey, RelayUrl};
use super::{MailboxCache, RelayEntry};
use super::inbox_helper::route_p_tags_to_inbox;

/// Route a `#p`-only interest (no authors/addresses) to inbox relays.
///
/// Passes an empty `authors_for_inbox` set because there is no author
/// constraint — the interest matches any event tagging the specified pubkeys.
/// The per-pubkey `#p` scoping in `route_p_tags_to_inbox` still applies:
/// Bob's relay sees only `#p:[Bob]`, not the full set of tagged pubkeys.
pub(super) fn route(
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    lifecycle: &InterestLifecycle,
    interest_id: &InterestId,
    mailbox_cache: &dyn MailboxCache,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // No `authors` (the Case A guard ensures this) — pass an empty set so
    // the inbox shape doesn't constrain authors.
    let empty_authors: BTreeSet<Pubkey> = BTreeSet::new();
    route_p_tags_to_inbox(
        p_tag_values,
        &empty_authors,
        base_shape,
        lifecycle,
        interest_id,
        mailbox_cache,
        relay_entries,
    );
}
