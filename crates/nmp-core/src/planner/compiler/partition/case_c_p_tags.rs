//! Case C: `#p` tag values → Inbox.
//!
//! Structural ban: `#p` interests MUST route to the inbox relay set selected
//! by the interest. Generic `#p` interests use NIP-65 read relays; NIP-17
//! gift-wrap inbox interests use kind:10050 DM relays. We never route to the
//! author's write relays, and we do not fall back to the indexer set.
//!
//! When inbox relays are unknown, we emit NO relay entries (fail-closed) and
//! emit a probe so the next recompile has data. The plan will have an empty
//! `per_relay` map for this interest until kind:10002 or kind:10050 arrives.
//!
//! ## PD-033-C planner extension (precursor to Stage 2 — `#p` bootstrap)
//!
//! The sibling [`route_bootstrap_content_inbox`] helper handles the cold-start
//! case where the kernel-driven self-zap-receipts subscription
//! (`kernel/requests/startup.rs`: `kind:9735 #p=[self_pk]` on
//! `RelayRole::Content`) needs to fly BEFORE the active account's kind:10002
//! arrives. Pre-PD-033-C the M1 `req(Content, …)` helper unconditionally
//! emitted the REQ on `bootstrap_urls_for_role(RelayRole::Content)`; the
//! planner mirror routes the equivalent `LogicalInterest` shape to
//! `bootstrap_content_relays` exactly when:
//!
//! - `lifecycle == Tailing`
//! - `scope == Global`
//! - `p_tag_routing == Nip65ReadRelays` (NIP-17 DM relays remain fail-closed
//!   by design — gift-wraps must NEVER leak to a non-DM relay)
//! - EVERY tagged pubkey has NO NIP-65 inbox cached (`get(pk)` is `None`, or
//!   the snapshot's `has_inbox_relays()` returns `false`)
//! - `bootstrap_content_relays` is non-empty
//!
//! When kind:10002 later arrives for any of the tagged pubkeys, the next
//! recompile naturally re-routes (the gate evaluates false because at least
//! one pubkey now has an inbox), and `plan_diff` emits a CLOSE on the
//! bootstrap relay paired with a REQ on the real inbox relay. No per-pubkey
//! narrowing on the bootstrap landing — the bootstrap content relay is a
//! single cold-start pad, not a per-recipient mailbox, so the original `#p`
//! set is preserved verbatim.
//!
//! The gate is the dispatcher's responsibility ([`super::partition_interest`]);
//! this helper assumes the gate already fired.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2,
//!          `docs/architecture-audit/pd033c-plan.md` §4.3
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestId, InterestLifecycle, InterestShape, LogicalInterest, Pubkey, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
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

/// PD-033-C planner extension: route a `Tailing + Global + #p` interest to
/// `bootstrap_content_relays` when every tagged pubkey lacks a cached
/// NIP-65 inbox.
///
/// Mirrors M1's `req(RelayRole::Content, …)` cold-start emission for the
/// kernel's self-zap-receipts subscription (`kind:9735 #p=[self_pk]`,
/// `kernel/requests/startup.rs`). Without this, deleting the M1 helper would
/// silently lose every #p-tagged Tailing REQ until kind:10002 arrives —
/// breaking the F-04 zap-receipts contract on cold-start sign-ins.
///
/// Gating happens at the dispatcher ([`super::partition_interest`]); this
/// helper assumes the caller has already verified:
/// - `lifecycle == Tailing` AND `scope == Global`
/// - `p_tag_routing == Nip65ReadRelays` (NIP-17 DM relays must NEVER divert
///   to a non-DM relay)
/// - every tagged pubkey has no cached NIP-65 inbox
/// - `bootstrap_content_relays` is non-empty
///
/// All emitted entries are tagged
/// `RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap)` so
/// diagnostics distinguish cold-start bootstrap routing from regular inbox
/// routing once mailboxes arrive.
///
/// The original `#p` tag set is preserved verbatim in `base_shape.tags` —
/// no per-pubkey narrowing happens on the bootstrap landing because the
/// bootstrap relay is a shared cold-start pad, not a per-recipient mailbox.
///
/// Signature mirrors [`super::case_d_no_author::route_bootstrap_content`]
/// (the Stage 1 sibling) verbatim: `&LogicalInterest` in, four-lane relay
/// accumulator out — no intermediate wrapper types. Symmetry with the
/// existing helper is the readability invariant.
pub(super) fn route_bootstrap_content_inbox(
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
            .insert(RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap));
    }
    for (relay_url, sources) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: BTreeSet::new(),
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }
}

/// Predicate: every pubkey in `p_tag_values` has NO cached NIP-65 inbox.
///
/// The gate's pre-condition for [`route_bootstrap_content_inbox`]. Returns
/// `true` when EVERY tagged pubkey's `mailbox_cache.get(pk)` is `None` OR the
/// snapshot's `has_inbox_relays()` returns `false`. If ANY tagged pubkey has
/// an inbox cached, the regular `route` path can serve at least one recipient
/// and the bootstrap fallback must NOT fire (would over-fetch by routing to
/// both the real inbox AND the bootstrap relay).
///
/// Empty `p_tag_values` returns `true` vacuously, but the dispatcher's Case C
/// guard rules out that branch before either helper is called.
pub(super) fn every_tagged_pubkey_lacks_nip65_inbox(
    p_tag_values: &BTreeSet<Pubkey>,
    mailbox_cache: &dyn MailboxCache,
) -> bool {
    p_tag_values.iter().all(|pk| match mailbox_cache.get(pk) {
        Some(snapshot) => !snapshot.has_inbox_relays(),
        None => true,
    })
}


#[cfg(test)]
#[path = "case_c_p_tags/tests.rs"]
mod tests;
