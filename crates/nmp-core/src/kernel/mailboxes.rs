//! Router-driven REQ-relay resolution + planner-side [`MailboxCache`] adapter.
//!
//! # Current design
//!
//! The kernel never reads NIP-65 mailbox data directly for REQ construction
//! or publish-relay resolution; every decision goes through the injected
//! `outbox_router` (`Arc<dyn OutboxRouter>`, see `docs/architecture/crate-boundaries.md`
//! §3 for the trait seam and §4 for the router's implementation ownership):
//!
//! * [`Kernel::build_routing_context`] stack-allocates a [`RoutingContext`]
//!   from the call site's `app_relays` (cold-start seed),
//!   `indexer_relays` (operator-configured, feeds lane 6), and a
//!   [`BlockedRelaySet`] snapshot ([`Kernel::snapshot_blocked_relays`]).
//! * [`Kernel::route_subscription_relays`] builds a one-shot
//!   [`LogicalInterest`] and calls `outbox_router.route_subscription`,
//!   returning the resolved, sorted+deduped URL set. The router's lane 1
//!   (NIP-65) resolves first; lane 7 (`AppRelayMode::Fallback`) fires from
//!   the seeded `app_relays` when lane 1 has nothing cached — see
//!   `docs/architecture/crate-boundaries.md` §5 for the full lane contract.
//! * [`Kernel::recipient_publish_relays`] drives the same router through
//!   `route_publish` with a synthetic [`UnsignedEvent`] so a downstream
//!   publisher (e.g. NIP-57's LN provider) can resolve a recipient's
//!   publish-side relay set without reading the cache directly.
//! * [`Kernel::bootstrap_seed_urls`] resolves the [`BootstrapSeed`]
//!   discriminant to the concrete cold-start URL list passed as
//!   `app_relays`. `BootstrapSeed::Discovery` is currently the only
//!   variant (indexer + content seeds combined); it is the lane-7 fallback
//!   for both content-direction and discovery-direction REQs.
//! * The DM-inbox lookup ([`Kernel::recipient_dm_relays`]) is the one
//!   exception to "everything through the router": it reads the injected
//!   [`DmInboxRelayLookup`] handle directly, because the kernel does not
//!   know the wire shape of a kind:10050 event and the router does not
//!   consult the DM-inbox cache. The gift-wrap publish path (`nmp-nip17`)
//!   uses `PublishTarget::Explicit` with the kind:10050 relay set instead
//!   of routing through the outbox router.
//! * The [`KernelMailboxes`] adapter bridges the substrate
//!   [`SubstrateMailboxCache`] + [`DmInboxRelayLookup`] handles to the
//!   planner's [`PlannerMailboxCache`] trait (see the adapter's own doc
//!   comment below for why two distinct `MailboxCache` traits are
//!   intentional, not a duplicate).

use std::sync::Arc;

use super::Kernel;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    MailboxCache as PlannerMailboxCache, MailboxSnapshot, Pubkey,
};
use crate::substrate::{
    BlockedRelaySet, DmInboxRelayLookup, MailboxCache as SubstrateMailboxCache, RoutingContext,
    SessionKeySet,
};
use crate::util::sort_dedup;
use nmp_signer_iface::UnsignedEvent;

impl Kernel {
    /// Snapshot the active account's blocked-relay set from the
    /// [`crate::substrate::BlockedRelayLookup`] handle. The returned
    /// [`BlockedRelaySet`] is stack-local — callers pass it to
    /// [`Self::build_routing_context`] and drop it at the end of the
    /// routing call (no `Arc`s held across awaits / actor ticks).
    ///
    /// When no active account is set (cold-start, post-logout), or the
    /// account has never declared any blocks, returns an empty set — the
    /// router's subtractive blocked-set post-pass is a no-op in either
    /// case, matching the pre-V-40 four `BlockedRelaySet::new()` call
    /// sites byte-for-byte.
    pub(crate) fn snapshot_blocked_relays(&self) -> BlockedRelaySet {
        match self.active_account.as_deref() {
            Some(pk) => self.blocked_relays_arc().blocked_relays(pk),
            None => BlockedRelaySet::new(),
        }
    }

    /// Resolve a pubkey's DM-inbox relays through the substrate
    /// [`DmInboxRelayLookup`] handle.
    ///
    /// The concrete cache (NIP-17 kind:10050) lives in `nmp-nip17` and is
    /// injected at composition time via
    /// [`Kernel::set_dm_inbox_relay_lookup`] (V-40); the kernel never names
    /// the NIP-17 wire shape (D0).
    ///
    /// Returns `None` when no list is known for `pubkey` — by trait
    /// contract this collapses both the "never published" and "published
    /// an empty list" branches, so the gift-wrap publish path fails
    /// closed in both cases (the contract NIP-17 § 2 requires). The
    /// router never sees this — DM gift-wrap routes via
    /// `PublishTarget::Explicit` in `nmp-nip17::dm_send`.
    pub(crate) fn recipient_dm_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        self.dm_inbox_relays_arc().dm_inbox_relays(pubkey)
    }
}

// ─── Router-driven REQ-relay resolution (Debt A) ─────────────────────────────
//
// These helpers replace the pre-Debt-A `author_write_relays` /
// `recipient_read_relays` / `author_indexer_relays` /
// `partition_ids_by_author_write_relays` cache-read helpers as the
// kernel's REQ-construction surface. The kernel's `outbox_router` slot
// is the live decision authority for every kernel-driven REQ; the
// returned URL set is consumed by `requests/profile.rs`. Author/thread view
// feeds now route through app-owned FlatFeed registrations plus `open_interest`.

/// Discriminator for the cold-start bootstrap seed passed into
/// `app_relays` at the [`RoutingContext`] construction site.
///
/// `Discovery` is the combined indexer + content seed (used for
/// content-direction REQs: timeline kind:1/6, hashtag firehose, thread
/// hydration). `IndexerOnly` is the indexer-lane seed (used for
/// discovery-direction REQs: kind:0 profile claims, kind:10002 NIP-65
/// probes). The router's lane 7 fires identically in both cases — only
/// the cold-start URL set differs.
#[derive(Clone, Copy)]
pub(crate) enum BootstrapSeed {
    /// Indexer + content seeds combined (matches the historical
    /// [`Kernel::bootstrap_discovery_relays`] output).
    Discovery,
}

impl Kernel {
    /// Resolve the kernel's cold-start bootstrap seed for a given
    /// direction. Returns the URL set the kernel passes through
    /// [`SessionKeySet::app_relays`] for the lane 7 fallback.
    pub(crate) fn bootstrap_seed_urls(&self, seed: BootstrapSeed) -> Vec<String> {
        match seed {
            BootstrapSeed::Discovery => self.bootstrap_discovery_relays(),
        }
    }

    /// Build a [`RoutingContext`] from the kernel's substrate state and
    /// the supplied bookkeeping references. The lifetime of the returned
    /// context is tied to the borrows in `app_relays` / `indexer_relays`
    /// / `blocked` — callers stack-allocate all three then drop the
    /// context before the next kernel-mutating call.
    ///
    /// `indexer_relays` is the operator-configured indexer URL set the
    /// router consults for spec §3.1 lane 6 (discovery-kind always-on
    /// stacking). It must be populated for kind:0 / kind:3 / kind:
    /// 10000–19999 routing to defeat the kind:10002 self-sealing loop
    /// (V-50); production wires it from
    /// `Kernel::bootstrap_urls_for_role(RelayRole::Indexer)`.
    pub(crate) fn build_routing_context<'a>(
        &'a self,
        app_relays: &'a [String],
        indexer_relays: &'a [String],
        blocked: &'a BlockedRelaySet,
    ) -> RoutingContext<'a> {
        RoutingContext {
            active_account: self.active_account.as_ref(),
            session_keys: SessionKeySet {
                app_relays,
                indexer_relays,
                ..SessionKeySet::default()
            },
            mailbox_cache: &*self.mailbox_cache,
            blocked_relays: blocked,
        }
    }

    /// Route a one-shot subscription for the given authors + kinds
    /// through the kernel's `outbox_router` and return the resolved
    /// URL set (sorted + deduped). The router's trace observer fires
    /// on success.
    ///
    /// `seed` selects the cold-start bootstrap URL set passed via
    /// [`SessionKeySet::app_relays`] — the router's lane 7 fires when
    /// lane 1 (NIP-65 cache) returns nothing.
    ///
    /// `interest_id` is the stable [`InterestId`] the trace projection
    /// surfaces (`chirp-repl routing-trace`, the iOS inspector); each
    /// call site derives a `stable_hash64` over its sub-id seed so a
    /// re-dispatch maps to the same row.
    ///
    /// On `RoutingError::Unroutable` (no cache hit, no AppRelay seed):
    /// returns an empty vec. The kernel's caller emits no REQ in that
    /// case — the failure surfaces via the trace projection's absence
    /// of a row, exactly the same observability shape the pre-Debt-A
    /// observer recorded.
    pub(crate) fn route_subscription_relays(
        &self,
        interest_id: u64,
        authors: &[&str],
        kinds: &[u32],
        seed: BootstrapSeed,
    ) -> Vec<String> {
        let shape = InterestShape {
            authors: authors.iter().map(|s| (*s).to_string()).collect(),
            kinds: kinds.iter().copied().collect(),
            ..InterestShape::default()
        };
        let interest = LogicalInterest {
            id: InterestId(interest_id),
            scope: InterestScope::Global,
            shape,
            hints: vec![],
            lifecycle: InterestLifecycle::OneShot,
            // The kernel-driven discovery-direction REQs (profile claim,
            // NIP-65 probe, contacts) are exactly the bootstrap-indexer
            // fallback's reason to exist — opt in so case_a_authors routes
            // them through `bootstrap_indexer_relays` when the author
            // mailbox is unknown.
            is_indexer_discovery: true,
        };
        let app_relays = self.bootstrap_seed_urls(seed);
        // V-50: indexer URLs feed router lane 6 (always-on for discovery
        // kinds). Cheap to populate unconditionally — the router only
        // consults the slice when `is_discovery_kind` matches.
        let indexer_relays = self.bootstrap_urls_for_role(nmp_network::role::RelayRole::Indexer);
        let blocked = self.snapshot_blocked_relays();
        let ctx = self.build_routing_context(&app_relays, &indexer_relays, &blocked);
        match self.outbox_router.route_subscription(&interest, &ctx) {
            Ok(routed) => {
                let mut out: Vec<String> = routed.urls().cloned().collect();
                sort_dedup(&mut out);
                out
            }
            Err(_) => Vec::new(),
        }
    }

    /// Resolve the relay URLs a downstream publisher (NIP-57 LN provider,
    /// etc.) should publish a `kind`-typed event authored by `recipient`
    /// to, via the kernel's `outbox_router` slot. Drives the router with a
    /// synthetic publish-direction [`UnsignedEvent`] so lane 1 returns the
    /// recipient's NIP-65 write set; lane 6 stacks the indexer URLs when
    /// `kind` is a discovery kind; lane 7 fires the Discovery cold-start
    /// seed when neither earlier lane resolved anything.
    ///
    /// This is the substrate seam the [`crate::substrate::RecipientRelayLookup`]
    /// capability is wired through. The Debt-C-follow-up replaced the
    /// pre-Debt-C `author_write_relays` bare cache accessor that
    /// `nmp-nip57::lnurl::inject_recipient_relays` consumed — the routing
    /// decision now belongs to the router, not a cache read.
    ///
    /// Returns an empty `Vec` on `RoutingError::Unroutable` (no NIP-65
    /// cache hit, no AppRelay seed) — caller (the LNURL fetcher) decides
    /// whether to surface an empty `relays` tag or fall back further.
    pub(crate) fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        let synthetic = UnsignedEvent {
            pubkey: recipient.to_string(),
            kind,
            tags: vec![],
            content: String::new(),
            created_at: 0,
        };
        let app_relays = self.bootstrap_seed_urls(BootstrapSeed::Discovery);
        let indexer_relays = self.bootstrap_urls_for_role(nmp_network::role::RelayRole::Indexer);
        let blocked = self.snapshot_blocked_relays();
        let ctx = self.build_routing_context(&app_relays, &indexer_relays, &blocked);
        match self.outbox_router.route_publish(&synthetic, &ctx) {
            Ok(routed) => {
                let mut out: Vec<String> = routed.urls().cloned().collect();
                sort_dedup(&mut out);
                out
            }
            Err(_) => Vec::new(),
        }
    }
}

// ─── KernelMailboxes adapter (T132) ──────────────────────────────────────────

/// Adapter — present the substrate [`SubstrateMailboxCache`] (NIP-65
/// kind:10002, owned by the kernel via `mailbox_cache`) plus the
/// substrate [`DmInboxRelayLookup`] handle (DM-inbox relays — NIP-17
/// kind:10050 in practice, but unnamed at this seam) as a planner-side
/// [`PlannerMailboxCache`].
///
/// Two traits, one bridge — and the bridge is the **durable** resolution,
/// not a temporary shim (#967). The planner trait (`get` → `MailboxSnapshot`
/// with read/write/both *separate*, plus `dm_inbox_relays`, `generation`,
/// `request_probe`) lives in Layer-2 `nmp-planner`, which MUST NOT depend on
/// Layer-3 `nmp-core`; the substrate `MailboxCache` is NIP-65-only and lives
/// here. The two CANNOT collapse into one trait without a forbidden
/// `nmp-planner -> nmp-core` dependency inversion, so this adapter — owned by
/// the one crate that legally sees both layers — is the single, permanent
/// point of translation. (See `substrate/routing.rs` module docs.)
///
/// Lifetime: holds an `Arc` clone of each substrate handle (cheap — both
/// are already `Arc<dyn …>`). The adapter is built per
/// `drain_lifecycle_tick` call and dropped at the end of that call.
pub(crate) struct KernelMailboxes {
    inner: Arc<dyn SubstrateMailboxCache>,
    dm_lookup: Arc<dyn DmInboxRelayLookup>,
}

impl KernelMailboxes {
    /// Constructor is kernel-private — outside callers obtain a view
    /// through [`Kernel::drain_lifecycle_tick`].
    pub(super) fn new(
        inner: Arc<dyn SubstrateMailboxCache>,
        dm_lookup: Arc<dyn DmInboxRelayLookup>,
    ) -> Self {
        Self { inner, dm_lookup }
    }
}

impl PlannerMailboxCache for KernelMailboxes {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.inner.snapshot(pubkey).map(|p| MailboxSnapshot {
            write_relays: p.write,
            read_relays: p.read,
            both_relays: p.both,
        })
    }

    fn dm_inbox_relays(&self, pubkey: &Pubkey) -> Option<Vec<String>> {
        self.dm_lookup.dm_inbox_relays(pubkey)
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.inner
            .snapshot_all()
            .into_iter()
            .map(|(pk, p)| {
                (
                    pk,
                    MailboxSnapshot {
                        write_relays: p.write,
                        read_relays: p.read,
                        both_relays: p.both,
                    },
                )
            })
            .collect()
    }

    fn generation(&self) -> u64 {
        // Phase 1: no generation counter on the substrate cache. Plan-id
        // stability is preserved at the kernel call site: the registered
        // kind:10002 parser mutates the substrate cache, and the projection
        // sweep triggers a recompile only when that cache changed.
        0
    }
}
