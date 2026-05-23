//! Case A: explicit `authors` → Outbox (write relays).
//!
//! When an interest carries both `authors` AND `#p` tag values, this case
//! emits Outbox entries AND calls `inbox_helper::route_p_tags_to_inbox` for
//! the "both populated" split (spec §3.1 "Both populated" row).
//!
//! ## Routing rules (post-T134 clarification)
//!
//! - Author with known NIP-65 mailbox → REQ goes to the UNION of the author's
//!   `outbox_relays()` and the kernel-configured `app_relays`. Both lanes are
//!   recorded on the resulting `RelayEntry::sources` so diagnostics see
//!   `{Nip65, UserConfigured(AppRelay)}` for any URL that landed in both
//!   sets.
//! - Author with NO NIP-65 mailbox → REQ goes to `app_relays` ONLY, with
//!   lane `UserConfigured(AppRelay)`. We still emit `request_probe` so that
//!   kind:10002 lookup populates the mailbox cache and the next recompile
//!   routes the author through NIP-65.
//! - Author with NO NIP-65 mailbox AND no `app_relays` configured AND the
//!   interest is `OneShot + Global` → REQ goes to `bootstrap_indexer_relays`
//!   with lane `UserConfigured(Indexer)`. This is the PD-033-C planner-
//!   extension arm (`docs/architecture-audit/pd033c-plan.md` §4.3): kernel-
//!   driven discovery oneshots for referenced pubkeys
//!   (`kernel/discovery.rs::drain_unknown_oneshots`'s profile-oneshot arm) fan
//!   to `RelayRole::Indexer` for kind:0/3/10002 lookups, so the planner must
//!   mirror that decision for the equivalent `LogicalInterest`.
//!   `bootstrap_indexer_relays` is the WITH-FALLBACK form (carries
//!   `FALLBACK_INDEXER_RELAY` when no indexer row is configured yet), matching
//!   `Kernel::bootstrap_urls_for_role(RelayRole::Indexer)` byte-for-byte —
//!   crucial so cold-start sign-ins (no rows yet) don't lose discovery REQs
//!   the moment Stage 1 deletes the M1 helper. The raw `indexer_relays` field
//!   (no fallback) is INTENTIONALLY not consulted here; using it would
//!   silently disable discovery whenever the operator hadn't yet configured
//!   an indexer row.
//! - Author with NO NIP-65 mailbox AND no `app_relays` AND NOT a `OneShot +
//!   Global` interest → the author is recorded in `unroutable` so the kernel
//!   can surface a UI diagnostic. The interest still flies to other authors'
//!   relays.
//!
//! Outside the PD-033-C `OneShot + Global` arm the indexer set is NEVER
//! consulted in this case — indexers remain discovery-only for tailing
//! follow-feed authors (the T134 invariant), and the kernel surfaces missing
//! mailboxes as "unroutable" via the UI toast as before.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestScope, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayUrl},
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
///
/// `bootstrap_indexer_relays` is the PD-033-C planner-extension fallback for
/// the `OneShot + Global` discovery-oneshot arm only — see module doc and the
/// `if !landed` block below. Tailing / account-scoped interests never touch
/// it. The raw `indexer_relays` field is deliberately not threaded in here
/// (cold-start divergence: `bootstrap_indexer_relays` carries
/// `FALLBACK_INDEXER_RELAY` when rows are empty; raw `indexer_relays` does
/// not).
//
// `too_many_arguments` allowed: this is a crate-internal routing helper whose
// parameters mirror the public compiler context plus its two accumulators;
// repackaging them behind a struct would obscure the dispatch in
// `partition::partition_interest` for no readability gain.
#[allow(clippy::too_many_arguments)]
pub(super) fn route(
    interest: &LogicalInterest,
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    app_relays: &[RelayUrl],
    bootstrap_indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    unroutable: &mut BTreeSet<Pubkey>,
) {
    // PD-033-C: gates the kernel-driven discovery-oneshot fallback. The two
    // conjuncts (`OneShot` + `Global`) intentionally match
    // `kernel/discovery.rs::drain_unknown_oneshots`'s shape exactly —
    // `oneshot.request(registry, InterestScope::Global, shape)` always
    // constructs an interest with `lifecycle: OneShot` (see
    // `subs/oneshot.rs::request`). Account-scoped profile fetches and tailing
    // follow-feed interests both fail this gate and retain their
    // pre-PD-033-C unroutable behaviour.
    let is_discovery_oneshot = matches!(interest.lifecycle, InterestLifecycle::OneShot)
        && matches!(interest.scope, InterestScope::Global);
    // Accumulate per-relay (authors, addresses, sources) before pushing
    // RelayEntry objects. This lets multiple authors share a relay without
    // creating separate entries — Stage 3 merge operates on the combined set.
    //
    // The per_relay map accumulates ALL RoutingSource lanes per relay URL.
    // Author A may reach wss://x via NIP-65 while author B reaches the same
    // relay via AppRelay (because the operator pinned the same URL); both
    // lanes must be recorded so Stage 3 role_tags reflects the four-lane
    // model (§3.1 four-lane discipline).
    let mut per_relay: BTreeMap<RelayUrl, CaseAEntry> = BTreeMap::new();

    for author in &interest.shape.authors {
        // Track whether ANY relay entry was emitted for this author. A known
        // mailbox with an empty `outbox_relays()` and no `app_relays` is just
        // as unroutable as a missing mailbox — in both cases zero REQs go out
        // for this author, which is exactly the condition the kernel surfaces
        // a toast for. The strict per-author landing-pad check captures both.
        let mut landed = false;

        match mailbox_cache.get(author) {
            Some(snapshot) => {
                // NIP-65 lane: author's declared write relays.
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.0.insert(author.clone());
                    entry.2.insert(RoutingSource::Nip65);
                    landed = true;
                }
            }
            None => {
                // No mailbox known — probe so cache can be populated and
                // the next recompile routes via NIP-65 (§3.2). Probing is
                // independent of whether app_relays carry this author now;
                // a NIP-65 update later moves them onto their own write set.
                mailbox_cache.request_probe(author);
            }
        }

        // AppRelay lane: additive whenever app_relays are configured, both
        // when NIP-65 is known AND when it is unknown.
        for relay in app_relays {
            let entry = per_relay
                .entry(relay.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
            entry.0.insert(author.clone());
            entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
            landed = true;
        }

        if !landed {
            // PD-033-C planner extension: a `OneShot + Global` interest whose
            // author has no NIP-65 mailbox AND no app_relays falls back to
            // `bootstrap_indexer_relays` instead of being marked unroutable.
            // This matches `kernel/discovery.rs::drain_unknown_oneshots`'s
            // profile-oneshot arm which fans the equivalent kind:0/3/10002
            // filter to `RelayRole::Indexer` today (the kernel calls
            // `bootstrap_urls_for_role(RelayRole::Indexer)`, which includes the
            // `FALLBACK_INDEXER_RELAY` cold-start default — so on cold-start
            // sign-ins the discovery REQ still lands somewhere). Tailing
            // follow-feed interests are NOT eligible — they continue to land
            // in `unroutable` so the kernel can surface the toast.
            if is_discovery_oneshot && !bootstrap_indexer_relays.is_empty() {
                for relay in bootstrap_indexer_relays {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.0.insert(author.clone());
                    entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::Indexer));
                    landed = true;
                }
            }
            if !landed {
                unroutable.insert(author.clone());
            }
        }
    }

    for coord in &interest.shape.addresses {
        let mut landed = false;

        match mailbox_cache.get(&coord.pubkey) {
            Some(snapshot) => {
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.1.insert(coord.clone());
                    entry.2.insert(RoutingSource::Nip65);
                    landed = true;
                }
            }
            None => {
                mailbox_cache.request_probe(&coord.pubkey);
            }
        }

        for relay in app_relays {
            let entry = per_relay
                .entry(relay.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
            entry.1.insert(coord.clone());
            entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
            landed = true;
        }

        if !landed {
            // PD-033-C planner extension is intentionally NOT applied to the
            // address-pointer arm. The kernel-driven discovery oneshots in
            // `kernel/discovery.rs::drain_unknown_oneshots` only target
            // `event_ids` (content arm) and `authors` (profile arm) — never
            // `addresses`. Address-pointer hydration is a view-module
            // responsibility (e.g. `nmp_nip01::ThreadView`) and runs through
            // the regular Case A author lane via the coord's `pubkey`. Keeping
            // the unroutable behaviour here preserves the existing UI
            // diagnostic for addressable events with no NIP-65/app-relays —
            // exactly the pre-PD-033-C semantics.
            unroutable.insert(coord.pubkey.clone());
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

#[cfg(test)]
#[path = "case_a_authors/tests.rs"]
mod tests;
