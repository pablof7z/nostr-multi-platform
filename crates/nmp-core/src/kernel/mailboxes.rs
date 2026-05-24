//! NIP-65 cache-read helpers + planner-side [`MailboxCache`] adapter.
//!
//! Step 3 of `docs/architecture/crate-boundaries.md` (V-50) cuts the
//! kernel over to `Arc<dyn OutboxRouter>` + `Arc<dyn MailboxCache>`.
//! This file is the post-step-3 home of the survivors of the deleted
//! `kernel/outbox.rs`:
//!
//! - `author_write_relays` / `author_indexer_relays` /
//!   `recipient_read_relays` — cache-read helpers with bootstrap
//!   fallback policy. Read through [`Kernel::mailbox_cache`] (the
//!   substrate [`MailboxCache`] handle, which step 3 made the single
//!   source of truth for kind:10002 data); apply the kernel's
//!   bootstrap-discovery / indexer-seed fallback when the cache misses.
//!   Not routing decisions in the new model — those flow through
//!   [`crate::substrate::OutboxRouter`]. These helpers wrap "cache hit
//!   with fallback to the kernel-owned bootstrap seed" because the
//!   kernel owns the role-to-URL mapping (`RelayEditRow`); a Layer-2
//!   router doesn't.
//! - `recipient_dm_relays` — DM-inbox relay cache reader. Reads through
//!   the injected substrate [`DmInboxRelayLookup`] handle (V-40); the
//!   kernel does not know the wire shape of a kind:10050 event.
//! - `partition_ids_by_author_write_relays` — thread-hydration outbox
//!   path. Wraps `author_write_relays`.
//! - [`KernelMailboxes`] — the planner-side adapter that bridges the
//!   substrate [`crate::substrate::MailboxCache`] to the planner's own
//!   `MailboxCache` trait (different shape: separate read/write/both
//!   fields plus `dm_inbox_relays`). Both traits coexist until step 9
//!   extracts the planner.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Kernel;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    MailboxCache as PlannerMailboxCache, MailboxSnapshot, Pubkey,
};
use crate::relay::RelayRole;
use crate::substrate::{
    BlockedRelaySet, DmInboxRelayLookup, MailboxCache as SubstrateMailboxCache, RoutingContext,
    SessionKeySet,
};
use crate::util::sort_dedup;

impl Kernel {
    /// Resolve a single author's NIP-65 write relays (write + both markers).
    ///
    /// Reads through the injected substrate [`MailboxCache`]. Cold-start:
    /// no cached kind:10002 ⇒ the [`Kernel::bootstrap_discovery_relays`]
    /// seed (discovery interest only, per D3).
    pub(crate) fn author_write_relays(&self, author: &str) -> Vec<String> {
        match self.mailbox_cache().snapshot(&author.to_string()) {
            Some(parsed) if !parsed.write.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.write.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
        }
    }

    /// Resolve a single author's relays for **discovery** fetches (kind:0/3/10002).
    ///
    /// Cold-start: no cached kind:10002 ⇒ ONLY `INDEXER_RELAY_URL`.
    /// Unlike `author_write_relays`, the shared content relay is never
    /// included — profile-claim REQs must not go there. NIP-65 known:
    /// returns the author's declared write relays (they published kind:0
    /// there, so that is the right place to read it back).
    pub(crate) fn author_indexer_relays(&self, author: &str) -> Vec<String> {
        match self.mailbox_cache().snapshot(&author.to_string()) {
            Some(parsed) if !parsed.write.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.write.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_urls_for_role(RelayRole::Indexer),
        }
    }

    /// Resolve a single recipient's NIP-65 **read** relays (inbox direction —
    /// the relays a `#p`-tagged pubkey reads, where notifications/DMs land).
    ///
    /// Cold-start: no cached kind:10002 ⇒ the bootstrap discovery seed.
    ///
    /// T122 / codex R2: also serves the active account's hashtag firehose —
    /// the user is the recipient of their own hashtag interest, so the
    /// routing destination is their declared read relays.
    pub(crate) fn recipient_read_relays(&self, recipient: &str) -> Vec<String> {
        match self.mailbox_cache().snapshot(&recipient.to_string()) {
            Some(parsed) if !parsed.read.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.read.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
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
    /// closed in both cases (the contract NIP-17 § 2 requires).
    pub(crate) fn recipient_dm_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        self.dm_inbox_relays_arc().dm_inbox_relays(pubkey)
    }

    /// Partition `ids` by their **original-event author's** NIP-65 write
    /// relays — the thread hydration outbox path (T121, codex R1).
    ///
    /// For each id, look up the cached event in `self.events`. If found,
    /// route the id to every relay in the author's resolved write set.
    /// If the id is not in the local store (i.e. we have no record of
    /// who wrote it), route it to every bootstrap-discovery seed — the
    /// cold-start discovery path: that's the only socket we can ask
    /// "who wrote this id?" on without violating D3.
    ///
    /// D3 (outbox automatic): reply threads should not depend on
    /// bootstrap relays carrying the conversation — the original
    /// author's write relays are the canonical home of both their own
    /// event and (heuristically) the kind:1/6 replies that reference it
    /// via `#e`. Reply authors of course write to *their own* relays;
    /// routing reply-fetch to the root author's relays is a deliberate
    /// compromise: it converges on whichever relays already serve the
    /// thread context rather than fanning to every participant. See
    /// codex review R1 of T105 keystone for the rationale.
    ///
    /// Empty input yields an empty map (caller emits nothing).
    pub(crate) fn partition_ids_by_author_write_relays(
        &self,
        ids: &[String],
    ) -> BTreeMap<String, Vec<String>> {
        let mut by_relay: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let relays = match self.events.get(id) {
                Some(event) => self.author_write_relays(&event.author),
                None => self.bootstrap_discovery_relays(),
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
}

// ─── Live router invocation (V-51 phase 5) ───────────────────────────────────
//
// The cache-read helpers above stay in place: their `Vec<String>` return type
// carries a kernel-policy cold-start bootstrap-discovery fallback that the
// substrate router's lane 1 + lane 7 algorithm does not express (the router
// returns `Unroutable` rather than reaching for the bootstrap seed; the
// kernel keeps the cold-start fallback because the alternative is silently
// dropping discovery REQs on the first sign-in tick). They are intentionally
// not deleted.
//
// What was missing pre-V-51 phase 5: the kernel never *called* the injected
// `OutboxRouter`, so the routing-trace projection that the kernel auto-binds
// onto the default `Nip65WriteSetRouter` (and that production composition
// rebinds onto `nmp_router::GenericOutboxRouter` via `set_routing`) stayed
// empty across the whole live session — `chirp-repl routing-trace` rendered
// `<no recent subscriptions>` and the V-51 phase 4 smoke had to SKIP.
//
// The fix is the helper below: for each per-author subscription-dispatch
// site (`requests/profile.rs::author_requests`, `firehose_requests`,
// `profile_claim_request`, `pending_profile_claim_requests`,
// `requests/thread.rs::partition_ids_by_author_write_relays`'s
// per-id author lookup) the kernel constructs a synthetic
// `LogicalInterest` for the targeted author(s) and calls
// `self.outbox_router.route_subscription(...)` purely for observability —
// the returned `RoutedRelaySet` (or `RoutingError::Unroutable`) is dropped on
// the floor; the actual REQ-emission relays continue to flow through the
// cache-read helpers above so the cold-start bootstrap policy survives.
// The router observer (the kernel-owned `RoutingTraceProjection` clone, or
// any production observer threaded through `with_trace_observer`) fires
// inside the router call and the projection populates.

impl Kernel {
    /// Build a [`RoutingContext`] from the kernel's substrate state and
    /// the supplied bookkeeping references. The lifetime of the returned
    /// context is tied to the borrows in `app_relays` / `blocked` —
    /// callers stack-allocate both then drop the context before the next
    /// kernel-mutating call.
    ///
    /// `explicit_targets` is intentionally always `None` in the
    /// observability path: the kernel-internal helpers below dispatch the
    /// generic algorithm, not a NIP-crate override. NIP crates carrying
    /// `explicit_targets` already call `route_subscription` /
    /// `route_publish` directly through the substrate seam (`nmp-nip17`
    /// gift-wrap, `nmp-nip29` group actions).
    fn build_routing_context<'a>(
        &'a self,
        app_relays: &'a [String],
        blocked: &'a BlockedRelaySet,
    ) -> RoutingContext<'a> {
        RoutingContext {
            active_account: self.active_account.as_ref(),
            session_keys: SessionKeySet {
                app_relays,
                ..SessionKeySet::default()
            },
            mailbox_cache: &*self.mailbox_cache,
            blocked_relays: blocked,
            explicit_targets: None,
        }
    }

    /// V-51 phase 5 — fire the injected [`OutboxRouter`] for observability.
    ///
    /// Constructs a synthetic `LogicalInterest` carrying the supplied
    /// authors + kinds and invokes `route_subscription` through the
    /// kernel's `outbox_router` slot. The router's trace observer (the
    /// kernel-owned `RoutingTraceProjection` by default, swapped to a
    /// production observer if the host injected a router via
    /// `set_routing` with its own `with_trace_observer`) fires on
    /// success; the returned routed set is dropped on the floor.
    ///
    /// **Behaviour-neutral by design**: the caller's actual REQ emission
    /// continues to flow through `author_write_relays` /
    /// `recipient_read_relays` / `author_indexer_relays` because the
    /// substrate router's lane 1 + lane 7 algorithm does not express the
    /// kernel's cold-start bootstrap-discovery fallback (a NIP-65-miss
    /// author returns `RoutingError::Unroutable`, not the indexer seed).
    /// This helper only populates the trace projection so the V-51 phase 4
    /// validation harness, `chirp-repl routing-trace`, and the iOS
    /// inspector see live decisions.
    ///
    /// `Unroutable` errors are observed (and recorded as a kernel log
    /// line on the diagnostic firehose) but never surface as an error —
    /// the kernel's actual dispatch path is the source of truth for
    /// "did the REQ go out?".
    pub(crate) fn observe_subscription_through_router(
        &self,
        interest_id: u64,
        authors: &[&str],
        kinds: &[u32],
    ) {
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
        };
        // The kernel's live `app_relays` plumbing is the `bootstrap_discovery_relays()`
        // seed today (T122 / codex R2 — until a separate AppRelay lane lands
        // this is the closest analogue for the lane-7 fallback). Passing the
        // bootstrap seed in lets the router's lane 7 fire as a last-resort
        // attribution when an author has no NIP-65 yet, which is the same
        // structural attribution the planner records via
        // `UserConfiguredCategory::AppRelay`. Lane 1 (Nip65/Read) still wins
        // for any author whose kind:10002 was cached.
        let app_relays = self.bootstrap_discovery_relays();
        let blocked = BlockedRelaySet::new();
        let ctx = self.build_routing_context(&app_relays, &blocked);
        // Drop the result on the floor — the observer fired inside, that is
        // the whole point. The actual REQ emission is the cache-read helpers
        // (see module-level note above).
        let _ = self.outbox_router.route_subscription(&interest, &ctx);
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
