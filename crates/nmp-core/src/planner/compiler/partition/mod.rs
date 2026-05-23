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
//! - `case_a_authors`      — Case A: explicit authors → outbox relays
//! - `case_b_addresses`    — Case B: address-pointer pubkeys → outbox relays
//! - `case_c_p_tags`       — Case C: `#p` tag values → inbox relays (structural ban)
//! - `case_d_no_author`    — Case D: no author/address/p → active-account or indexer
//! - `case_e_relay_pinned` — Case E: `relay_pin` hard-pin → host relay only.
//!   Generic third routing lane; example consumer: NIP-29 relay-based groups.
//! - `inbox_helper`        — `route_p_tags_to_inbox` shared by Cases A and C

mod case_a_authors;
mod case_b_addresses;
mod case_c_p_tags;
mod case_d_no_author;
mod case_e_relay_pinned;
mod inbox_helper;

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{
        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
        PTagRouting, Pubkey, RelayUrl,
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
///   PD-033-C extension: a `OneShot + Global + event_ids`-shaped interest is
///   intercepted at the head of Case D and routed to `bootstrap_content_relays`
///   when that set is non-empty — the kernel-driven discovery oneshot path
///   that previously rode the M1 hand-rolled `req()` helper.
///
/// ## PD-033-C planner extension (§4.3)
///
/// Two narrow gates make discovery-oneshot interests routable without M1:
///
/// 1. **Case A `if !landed` fallback**: a `OneShot + Global` interest whose
///    author has no NIP-65 mailbox AND no `app_relays` falls through to
///    `indexer_relays` (lane `UserConfigured(Indexer)`) instead of being marked
///    `unroutable`. Mirrors `discovery.rs::drain_unknown_oneshots`'s
///    profile-oneshot arm which fans the same shape to `RelayRole::Indexer`.
/// 2. **Case D head**: a `OneShot + Global` interest with concrete `event_ids`
///    and no authors/addresses/p-tags routes to `bootstrap_content_relays`
///    (lane `UserConfigured(Bootstrap)`) when that set is non-empty — the
///    content-relay analogue of the indexer fallback for event-id discovery.
///
/// Both gates require `lifecycle == OneShot` AND `scope == Global` so they do
/// not perturb account-scoped profile fetches or tailing timeline interests.
//
// `too_many_arguments` allowed: this is the planner-private dispatcher; its
// parameter list is the compile-context surface (5 relay sets + mailbox cache
// + interest input) plus the two output accumulators. A struct wrapper would
// only force every call site through an extra builder for zero clarity gain.
#[allow(clippy::too_many_arguments)]
pub(super) fn partition_interest(
    interest: &LogicalInterest,
    mailbox_cache: &dyn MailboxCache,
    indexer_relays: &[RelayUrl],
    active_account_read_relays: &[RelayUrl],
    app_relays: &[RelayUrl],
    bootstrap_content_relays: &[RelayUrl],
    bootstrap_indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    unroutable: &mut BTreeSet<Pubkey>,
) {
    // `indexer_relays` is discovery-only (kind:0/3/10002) — never a content
    // fallback for Cases A–C. Case D consults them as a last-resort when both
    // active-account read relays and app relays are empty (hashtag firehose /
    // cold-start).

    let base_shape = InterestShape {
        authors: BTreeSet::new(),
        kinds: interest.shape.kinds.clone(),
        tags: interest.shape.tags.clone(),
        since: interest.shape.since,
        until: interest.shape.until,
        limit: interest.shape.limit,
        event_ids: interest.shape.event_ids.clone(),
        addresses: BTreeSet::new(),
        relay_pin: interest.shape.relay_pin.clone(),
        p_tag_routing: interest.shape.p_tag_routing,
    };

    // Case E (relay-pinned interest): hard-pin short-circuits the four-lane
    // dispatch entirely. Authors / addresses / #p on the same interest are
    // retained on the wire filter but ignored for routing. This is the
    // generic third routing lane — any protocol with single-host addressing
    // semantics can opt in by setting `relay_pin` on its `InterestShape`.
    if let Some(pin_url) = &interest.shape.relay_pin {
        case_e_relay_pinned::route(interest, &base_shape, pin_url, relay_entries);
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
            app_relays,
            bootstrap_indexer_relays,
            relay_entries,
            unroutable,
        );
        return;
    }

    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
    if !interest.shape.addresses.is_empty() {
        case_b_addresses::route(
            interest,
            &base_shape,
            mailbox_cache,
            app_relays,
            relay_entries,
            unroutable,
        );
        return;
    }

    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
    if !p_tag_values.is_empty() {
        // PD-033-C planner extension (Stage 2 precursor): `Tailing + Global +
        // #p (Nip65ReadRelays)` with EVERY tagged pubkey lacking a cached
        // NIP-65 inbox routes to `bootstrap_content_relays` BEFORE the
        // normal Case C body. This is the planner mirror of the M1
        // `req(RelayRole::Content, …)` emission for the kernel's
        // self-zap-receipts subscription (`kind:9735 #p=[self_pk]`,
        // `kernel/requests/startup.rs`) — without it, deleting the M1 helper
        // would silently lose every #p Tailing REQ until the active account's
        // kind:10002 lands (breaking the F-04 zap-receipts contract on
        // cold-start sign-ins). NIP-17 DM routing
        // (`p_tag_routing == Nip17DmRelays`) is intentionally excluded: those
        // subscriptions carry gift-wrapped private DMs and MUST stay
        // fail-closed when DM relays are unknown — diverting them to a
        // bootstrap content relay would leak gift-wraps to a non-DM relay.
        let is_bootstrap_inbox_eligible =
            matches!(interest.lifecycle, InterestLifecycle::Tailing)
                && matches!(interest.scope, InterestScope::Global)
                && matches!(interest.shape.p_tag_routing, PTagRouting::Nip65ReadRelays)
                && !bootstrap_content_relays.is_empty()
                && case_c_p_tags::every_tagged_pubkey_lacks_nip65_inbox(
                    &p_tag_values,
                    mailbox_cache,
                );
        if is_bootstrap_inbox_eligible {
            case_c_p_tags::route_bootstrap_content_inbox(
                interest,
                &base_shape,
                bootstrap_content_relays,
                relay_entries,
            );
            return;
        }
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

    // Case D: no authors, addresses, or #p → active-account read relays ∪
    // app relays (hashtag firehose). Indexer remains as a last-resort fallback
    // when BOTH sets are empty so the kernel-driven discovery REQs still have
    // somewhere to land in cold-start scenarios.
    //
    // PD-033-C head check: a `OneShot + Global` interest with concrete
    // `event_ids` is the kernel-driven discovery oneshot for referenced events
    // (`kernel/discovery.rs::drain_unknown_oneshots`). Route it to
    // `bootstrap_content_relays` BEFORE the existing accumulation so the
    // discovery REQ lands on a content relay (not the indexer set, which is
    // discovery-only for kind:0/3/10002). Non-discovery Case D interests
    // (Tailing firehose, Account-scoped reads, event_ids without `OneShot +
    // Global`) fall through to the unchanged routing below.
    let is_oneshot_global_event_ids_discovery = matches!(interest.lifecycle, InterestLifecycle::OneShot)
        && matches!(interest.scope, InterestScope::Global)
        && !interest.shape.event_ids.is_empty()
        && !bootstrap_content_relays.is_empty();
    if is_oneshot_global_event_ids_discovery {
        case_d_no_author::route_bootstrap_content(
            interest,
            &base_shape,
            bootstrap_content_relays,
            relay_entries,
        );
        return;
    }

    case_d_no_author::route(
        interest,
        &base_shape,
        active_account_read_relays,
        app_relays,
        indexer_relays,
        relay_entries,
    );
}
