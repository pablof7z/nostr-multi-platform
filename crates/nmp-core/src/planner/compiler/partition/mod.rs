//! `RelayEntry` and `partition_interest`: Stage 1+2 of the compiler pipeline.
//!
//! Partitions a single `LogicalInterest` into per-relay entries, with each
//! entry carrying only the authors that declared the relay (author-partitioning).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).
//!
//! ## Module layout (each sub-module ≤ 300 LOC)
//!
//! - `case_a_authors`   — Case A: explicit authors → outbox relays
//! - `case_b_addresses` — Case B: address-pointer pubkeys → outbox relays
//! - `case_c_p_tags`    — Case C: `#p` tag values → inbox relays (structural ban)
//! - `case_d_no_author` — Case D: no author/address/p → active-account or indexer
//! - `case_e_pin_to`    — Case E: `pin_to` hard-pin → host relay only (NIP-29)
//! - `inbox_helper`     — `route_p_tags_to_inbox` shared by Cases A and C

mod case_a_authors;
mod case_b_addresses;
mod case_c_p_tags;
mod case_d_no_author;
mod case_e_pin_to;
mod inbox_helper;

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{
        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
        RelayUrl,
    },
    plan::RoutingSource,
};
pub(super) use super::mailbox::MailboxCache;

// ─── RelayEntry ──────────────────────────────────────────────────────────────

/// A relay-partitioned slice of one logical interest.
///
/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
/// authors that declared this specific relay (not all N authors). This is the
/// author-partitioning that lets the merge lattice produce per-relay author
/// subsets.
///
/// `sources` is a set (not a single value) so that a relay reached by two
/// different lanes (e.g. NIP-65 for author A, Indexer for author B) preserves
/// both lanes in `role_tags` at Stage 3 (§3.1 four-lane discipline).
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
    /// All routing lanes that contributed to this relay entry.
    pub sources: BTreeSet<RoutingSource>,
    pub interest_id: InterestId,
}

impl RelayEntry {
    /// Construct the final `InterestShape` for this relay slice.
    pub fn into_shape(
        mut self,
    ) -> (InterestShape, InterestLifecycle, BTreeSet<RoutingSource>, InterestId) {
        self.base_shape.authors = self.authors_for_relay;
        self.base_shape.addresses = self.addresses_for_relay;
        (self.base_shape, self.lifecycle, self.sources, self.interest_id)
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
///   any `addresses` on the same interest to the same relay map. If the
///   interest also has `#p` tag values, inbox routing is emitted in addition
///   (see `inbox_helper::route_p_tags_to_inbox`; spec §3.1 "Both populated" row).
/// - **Case B**: no authors, but `addresses` → Outbox for each coord.pubkey.
/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
///   (tagged pubkey's read relays). Structural ban enforced: never route
///   `#p` interests to non-inbox relays. When inbox relays are unknown, the
///   interest produces NO relay entries (fail-closed); a probe is emitted so
///   the next recompile has data.
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
        pin_to: interest.shape.pin_to.clone(),
    };

    // Case E (NIP-29 host-relay-pin): hard-pin short-circuits the four-lane
    // dispatch entirely. Authors / addresses / #p on the same interest are
    // retained on the wire filter but ignored for routing. This is the third
    // routing lane required by `docs/design/nip29/routing.md` §3.
    if let Some(pin_url) = &interest.shape.pin_to {
        case_e_pin_to::route(interest, &base_shape, pin_url, relay_entries);
        return;
    }

    // Extract #p tag values once — used in Case A (if authors + #p) and Case C.
    let p_tag_values: BTreeSet<Pubkey> = interest
        .shape
        .tags
        .get("p")
        .cloned()
        .unwrap_or_default();

    // Case A: explicit authors → Outbox (write relays).
    if !interest.shape.authors.is_empty() {
        case_a_authors::route(
            interest,
            &p_tag_values,
            &base_shape,
            mailbox_cache,
            indexer_relays,
            relay_entries,
        );
        return;
    }

    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
    if !interest.shape.addresses.is_empty() {
        case_b_addresses::route(interest, &base_shape, mailbox_cache, indexer_relays, relay_entries);
        return;
    }

    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
    if !p_tag_values.is_empty() {
        case_c_p_tags::route(
            &p_tag_values,
            &base_shape,
            &interest.lifecycle,
            &interest.id,
            mailbox_cache,
            relay_entries,
        );
        return;
    }

    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
    case_d_no_author::route(
        interest,
        &base_shape,
        active_account_read_relays,
        indexer_relays,
        relay_entries,
    );
}
