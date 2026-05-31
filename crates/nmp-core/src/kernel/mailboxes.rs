//! Router-driven REQ-relay resolution + planner-side [`MailboxCache`] adapter.
//!
//! # V-50 / V-51 status
//!
//! Step 3 of `docs/architecture/crate-boundaries.md` cut the kernel over to
//! `Arc<dyn OutboxRouter>` + `Arc<dyn MailboxCache>` for *storage* but left
//! the kernel's REQ-construction sites reading the cache directly via
//! `author_write_relays` / `recipient_read_relays` / `author_indexer_relays`.
//! V-51 phase 5 (PR #462) added an observe-only `observe_subscription_through_router`
//! shim that fired the router for the trace projection but dropped the
//! routed set on the floor.
//!
//! **Debt A** (this commit) lifts the kernel's REQ-construction sites
//! onto the router as the live decision authority:
//!
//! * The substrate seam for the cold-start bootstrap seed is
//!   [`RoutingContext::session_keys::app_relays`] — the kernel populates it
//!   with the appropriate bootstrap list at each call site, and the router's
//!   existing lane 7 ([`crate::substrate::RoutingSource::AppRelay`] with
//!   [`crate::substrate::AppRelayMode::Fallback`]) handles "no NIP-65 cached"
//!   by falling back to that list. No new substrate field is required — the
//!   router's lane-1 → lane-7 algorithm already expresses the kernel's
//!   cold-start contract.
//! * Per-call helpers below ([`Kernel::route_subscription_relays`] and
//!   [`Kernel::partition_ids_via_router`]) construct the
//!   [`RoutingContext`], invoke `route_subscription` through the kernel's
//!   `outbox_router` slot, and return the routed URL set. The router's
//!   trace observer fires automatically on the success path — the
//!   `observe_subscription_through_router` half-step is gone.
//! * The DM-inbox lookup ([`Kernel::recipient_dm_relays`]) stays — it reads
//!   the injected [`DmInboxRelayLookup`] handle (V-40); the kernel does
//!   not know the wire shape of a kind:10050 event and the router does not
//!   consult the DM-inbox cache. The gift-wrap publish path
//!   (`nmp-nip17`) wires kind:10050 relays through `explicit_targets`.
//! * The [`KernelMailboxes`] adapter is unchanged — it bridges the
//!   substrate [`SubstrateMailboxCache`] + [`DmInboxRelayLookup`] handles
//!   to the planner's [`PlannerMailboxCache`] trait.
//!
//! # Discovery direction
//!
//! Profile-claim REQs (kind:0) and NIP-65 relay-list probes (kind:10002)
//! are *discovery-direction* reads: the cold-start seed must be the
//! indexer-only relay set (the shared content relay must never see those
//! probes, per `kernel/mailboxes.rs::author_indexer_relays` historical
//! semantics). The router does not yet implement lane 6 (Indexer
//! eligibility — `Nip65WriteSetRouter` carries the TODO); until it does,
//! the kernel selects the bootstrap seed per call site:
//!
//! * Content-direction (kind:1/6 timeline, hashtag firehose, thread
//!   hydration): `app_relays = bootstrap_discovery_relays()`
//!   (indexer + content seeds combined — same as
//!   [`Kernel::bootstrap_discovery_relays`]).
//! * Indexer-direction (kind:0 / kind:10002 / contacts probes):
//!   `app_relays = bootstrap_urls_for_role(Indexer)` only.
//!
//! Selecting the right seed at the call site is a kernel-level concern
//! (the kernel owns `configured_relays`); routing the seeded interest is a
//! router concern (lane 7 fires when lane 1 returned nothing). The seam
//! between them is the `app_relays` slot — exactly the shape the
//! substrate trait already exposes.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Kernel;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    MailboxCache as PlannerMailboxCache, MailboxSnapshot, Pubkey,
};
use crate::substrate::{
    BlockedRelaySet, DmInboxRelayLookup, MailboxCache as SubstrateMailboxCache, RoutingContext,
    SessionKeySet, UnsignedEvent,
};
use crate::util::sort_dedup;

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
    /// `explicit_targets` in `nmp-nip17::dm_send`.
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
// returned URL set is consumed by the call sites in
// `requests/profile.rs` and `requests/thread.rs`.

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
    /// Indexer seeds only — discovery-direction kind:0 / kind:10002 probes
    /// MUST NOT leak onto the shared content relay (cf. the pre-Debt-A
    /// `author_indexer_relays` `INDEXER_RELAY_URL`-only fallback contract).
    IndexerOnly,
}

impl Kernel {
    /// Resolve the kernel's cold-start bootstrap seed for a given
    /// direction. Returns the URL set the kernel passes through
    /// [`SessionKeySet::app_relays`] for the lane 7 fallback.
    pub(crate) fn bootstrap_seed_urls(&self, seed: BootstrapSeed) -> Vec<String> {
        match seed {
            BootstrapSeed::Discovery => self.bootstrap_discovery_relays(),
            BootstrapSeed::IndexerOnly => {
                self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer)
            }
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
            explicit_targets: None,
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
        let indexer_relays = self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
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

    /// Outbox-direction subscription resolution — route a single author's
    /// **write** set through the kernel's `outbox_router.route_publish`
    /// (with a synthetic [`UnsignedEvent`] carrying the author + the
    /// first of `kinds` as the kind discriminant). Returns the resolved
    /// URL set (sorted + deduped). The router's trace observer fires
    /// on success.
    ///
    /// The router's `route_subscription` shape resolves authors against
    /// the **read** lane (the inbox direction — "subscribe where the
    /// recipient reads"). Several kernel REQ-construction sites are
    /// *outbox-direction* instead: they fetch events from where the
    /// author *publishes*, not from where the recipient reads.
    /// Examples:
    ///
    /// * `author_requests::author_requests` — kind:1/6 author notes: the
    ///   author published those to their write relays (T105 outbox).
    /// * `author_requests::profile_claim_request` — kind:0 profile: the
    ///   author published their kind:0 to their write relays (D3 outbox
    ///   discovery).
    /// * `author_requests::author_requests` — kind:10002 NIP-65 probe:
    ///   the author published their kind:10002 to their write relays.
    ///
    /// For these the kernel calls `route_publish` with a synthetic event
    /// carrying the author's pubkey + the relevant kind so lane 1
    /// returns the *write* set. The actual event content / tags are
    /// immaterial — the router only reads `pubkey` and (in future
    /// lane 6) `kind`. `seed` selects the cold-start bootstrap URL set
    /// passed via `app_relays` (lane 7).
    ///
    /// `interest_id` is the stable [`InterestId`] the trace projection
    /// surfaces. Because the underlying call is `route_publish` the
    /// trace projection records a publish entry rather than a
    /// subscription entry — that is the semantically honest record
    /// ("the kernel asked the router 'where would `kind` from `author`
    /// land?'"). The kernel still emits a REQ on the resolved relays;
    /// the publish-trace classification refers to the *resolution
    /// algorithm*, not the wire frame.
    pub(crate) fn route_outbox_subscription_relays(
        &self,
        interest_id: u64,
        author: &str,
        kind: u32,
        seed: BootstrapSeed,
    ) -> Vec<String> {
        let synthetic = UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: vec![],
            content: String::new(),
            // `interest_id` is hashed from kernel-stable inputs; reusing
            // it as the synthetic `created_at` keeps the call deterministic
            // (the router doesn't read `created_at`, but logging /
            // tracing might).
            created_at: interest_id,
        };
        let app_relays = self.bootstrap_seed_urls(seed);
        // V-50: see `route_subscription_relays` comment — indexer URLs
        // populate lane 6 for discovery kinds. Outbox-direction
        // kind:10002 / kind:0 fetches need this too: the synthetic
        // event's `kind` field drives the lane check.
        let indexer_relays = self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
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

    /// Partition `ids` by the original-event author's NIP-65 write
    /// relays through the kernel's `outbox_router`. Used by thread
    /// hydration (`maybe_open_thread_hydration`): each id is looked up
    /// in `self.events`; if the author is found, a synthetic
    /// [`UnsignedEvent`] is constructed (kind 1 — the kind threads
    /// canonically carry) and the kernel's `outbox_router.route_publish`
    /// is invoked so lane 1 (NIP-65 write set) resolves the author's
    /// outbox relays. If the id is unknown (no event in the local
    /// store) the bootstrap discovery seed serves the cold-start
    /// lookup — the only socket we can ask "who wrote this id?" on
    /// without violating D3.
    ///
    /// T121 / codex R1: thread hydration is the named exception to the
    /// read-set algorithm — reply authors of course write to *their
    /// own* relays, but routing reply-fetch to the root author's relays
    /// is a deliberate compromise: it converges on whichever relays
    /// already serve the thread context rather than fanning to every
    /// participant. The router's `route_publish` shape (NIP-65 write
    /// set) is the right tool for this — we feed a synthetic kind:1
    /// `UnsignedEvent` per author to drive the publish-direction lane.
    ///
    /// Each id is added to every relay the router returns for its author.
    /// Empty input yields an empty map (caller emits nothing). The
    /// returned map keys are deterministic ([`BTreeMap`]) so plan-id
    /// stability is preserved (D8).
    pub(crate) fn partition_ids_via_router(&self, ids: &[String]) -> BTreeMap<String, Vec<String>> {
        let mut by_relay: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let bootstrap_app_relays = self.bootstrap_seed_urls(BootstrapSeed::Discovery);
        let indexer_relays = self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
        let blocked = self.snapshot_blocked_relays();
        for id in ids {
            let relays = match self.events.get(id) {
                Some(event) => {
                    let synthetic = UnsignedEvent {
                        pubkey: event.author.clone(),
                        kind: 1,
                        tags: vec![],
                        content: String::new(),
                        created_at: event.created_at,
                    };
                    let ctx = self.build_routing_context(
                        &bootstrap_app_relays,
                        &indexer_relays,
                        &blocked,
                    );
                    match self.outbox_router.route_publish(&synthetic, &ctx) {
                        Ok(routed) => {
                            let mut out: Vec<String> = routed.urls().cloned().collect();
                            sort_dedup(&mut out);
                            out
                        }
                        // `Unroutable` means the author has no NIP-65 AND
                        // no AppRelay seed was given (we passed the
                        // discovery seed, so this branch is unreachable
                        // in production — defensive vec to keep the loop
                        // total).
                        Err(_) => Vec::new(),
                    }
                }
                None => bootstrap_app_relays.clone(),
            };
            for relay in relays {
                by_relay.entry(relay).or_default().push(id.clone());
            }
        }
        // Stable id order within each relay slice (plan-id stability / D8).
        for ids in by_relay.values_mut() {
            sort_dedup(ids);
        }
        by_relay
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
        let indexer_relays = self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
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
/// Two traits, one bridge. The planner trait pre-dates the substrate
/// trait introduced in step 1.c / 1.d, and uses a different shape
/// (`get` → `MailboxSnapshot` with read/write/both *separate*, plus
/// `dm_inbox_relays`). Step 9 extracts the planner crate and the two
/// traits collapse into one then; until then this adapter is the
/// translation layer.
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
        // stability is preserved at the kernel call site (the kernel
        // triggers a recompile only when a kind:10002 actually mutated
        // the cache — see `ingest::relay_list::ingest_relay_list`'s
        // empty-vs-non-empty guard).
        0
    }
}
