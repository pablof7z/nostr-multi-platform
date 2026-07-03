//! Cold-start REQ emission: self profile / NIP-65 relay list / NIP-17 DM relay
//! list / kind:10006 blocked-relay list / kind:10007 search-relay list, and the
//! active account's kind:3 follow list. No hardcoded seed timeline.

use super::super::{Duration, Instant, Kernel, OutboundMessage};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use std::collections::BTreeSet;

/// Self-fetched account-config kinds the cold-start tailing subscription
/// keeps live after sign-in.
///
/// Reactive design: the host wires up its kind:0 / kind:3 / kind:10002 /
/// kind:10006 / kind:10007 readers exactly once at sign-in and gets fresh data
/// automatically whenever the account republishes any of these. The
/// pre-V-04 model fired a one-shot REQ per kind and closed it on EOSE,
/// which left apps stale after the first round-trip and forced ad-hoc
/// re-fetch loops in each view module.
///
/// - **0**: profile metadata
/// - **3**: contacts (follow list — the timeline depends on this staying
///   fresh as the user mutes / unfollows)
/// - **10002**: NIP-65 relay list (mailboxes — routing decisions must
///   re-resolve when the user edits this from a second device)
/// - **10006**: blocked-relay list (fed into the [`crate::substrate::BlockedRelayLookup`]
///   handle so the router's subtractive blocked-set post-pass picks up
///   changes mid-session)
/// - **10007**: search-relay list (NIP-51 — fed into the host-side
///   `SearchRelayListProjection` in `nmp-nip51` so transparent NIP-50 search
///   (`open_search(UserPreferred)`) can fan out to the user's preferred
///   search relays. Like kind:10006, this is an account-specific replaceable
///   list whose self-fetch must route + survive account switches; routing it
///   here through the proven tailing bundle is what makes the projection
///   populate with zero app involvement. The kernel learns NO NIP-51 nouns —
///   it only tails a kind number, exactly as it does for 10006.)
///
/// kind:10000 (mute list) is intentionally excluded: the host-side
/// `MuteRuntimeController` (in `explicit composition`) owns a dedicated
/// `authors=[active_pubkey] / kinds=[10000]` interest and pushes it via
/// `EnsureInterest` on sign-in. Free-riding on this bundle would route mute
/// lists through the wrong interest scope — D0 forbids the kernel knowing
/// about NIP-51 mute semantics.
const SELF_KINDS_TAILING: &[u32] = &[0, 3, 10002, 10006, 10007];

/// Self-fetched account-config kinds that must remain discovery one-shots even
/// though they are governed by the same host override as the tailing set.
const SELF_KINDS_ONESHOT: &[u32] = &[10050];

impl Kernel {
    pub(crate) fn startup_requests(&mut self, now: Instant) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(now + Duration::from_secs(3));
        self.active_account_bootstrap_requests()
    }

    /// Emit profile + relay-list + DM-relay-list + contacts REQs for the
    /// currently active account. Called at cold-start (via `startup_requests`)
    /// and again after sign-in / account creation / switch when the active
    /// account changes.
    ///
    /// F-02: kind:10050 (NIP-17 DM relay list) is fetched here so that
    /// existing users see their DM inbox subscription open immediately on
    /// sign-in instead of waiting for the DM runtime to publish its own
    /// kind:10050 and round-trip it back through the relay. Without this,
    /// `dm_relay_lists` is empty at sign-in and the `PTagRouting::Nip17DmRelays`
    /// routing for the gift-wrap inbox interest fails-closed until the
    /// publish→ingest round-trip closes — a structural latency wart for any
    /// user who already has a kind:10050 published on a prior device.
    ///
    /// V-04 Stage 2: bootstrap interests are registered through
    /// [`crate::subs::InterestRegistry::ensure_sub`] instead of being emitted
    /// as M1 `self.req(...)` frames. The planner's next `drain_tick` compiles
    /// them into wire REQs against `bootstrap_indexer_relays` (the planner
    /// extension's fallback lane for `OneShot + Global + authors` shapes
    /// without an NIP-65 mailbox — see
    /// `planner/compiler/partition/case_a_authors.rs`'s `is_discovery_oneshot`
    /// gate). The returned `Vec<OutboundMessage>` is empty; callers extend
    /// with it as a zero-cost no-op. The native actor's idle loop calls
    /// `drain_lifecycle_tick` on the next tick; the wasm `KernelReducer` calls
    /// `drain_lifecycle_outbound` inline from `handle_relay_connected`.
    pub(crate) fn active_account_bootstrap_requests(&mut self) -> Vec<OutboundMessage> {
        let self_pk = match &self.active_account {
            Some(pk) => pk.clone(),
            None => return Vec::new(),
        };

        // Owner is a single stable `"kernel:bootstrap"` slot so the per-kind
        // interests all share one owner refcount but stay distinct via their
        // [`SubKey`]s.
        //
        // Account-switch eviction: each bootstrap call uses `set_sub` (NOT
        // `ensure_sub`) so the slot's author cell is **replaced** with
        // `self_pk` for the new active account. `SubKey::new(seed)` is
        // intentionally account-independent (the seed strings are static),
        // so without `set_sub` the prior account's authors would persist
        // in the registry across account switches — the silent privacy /
        // staleness leak the V-04 design called out. The `(scope, key)`
        // slot survives the replacement; only the inner `LogicalInterest`
        // mutates.
        let owner = SubOwnerKey::new("kernel:bootstrap");

        let selected_self_kinds = self.selected_bootstrap_self_kinds();

        // ── Discovery-direction one-shots (kind:10050 only) ───────────────
        //
        // kind:10050 (NIP-17 DM relay list) intentionally stays a OneShot:
        // it is consumed by the DM gift-wrap publish path on demand, the
        // recipient's `dm_inbox_relays` cache is a read-once snapshot, and
        // tailing it would multiply REQ pressure on the indexer for no
        // observable behavioural win. The host override still governs whether
        // this lane exists: omitting 10050 from the override opts out.
        //
        // `is_indexer_discovery: true` opts the interest into
        // `case_a_authors`'s `bootstrap_indexer_relays` fallback so the
        // cold-start author-unknown case lands a REQ instead of falling
        // through to `unroutable`.
        let oneshot_self_kinds = selected_self_kinds
            .iter()
            .copied()
            .filter(|kind| SELF_KINDS_ONESHOT.contains(kind))
            .collect::<BTreeSet<_>>();
        if !oneshot_self_kinds.is_empty() {
            self.register_oneshot_discovery_interest(
                owner,
                "bootstrap:self-dm-relays",
                oneshot_self_kinds,
                self_pk.clone(),
            );
        }

        // ── Reactive tailing self-kind subscription ──────────────────────
        //
        // One Tailing interest carrying every account-config kind in
        // `SELF_KINDS_TAILING` (kinds 0, 3, 10002, 10006, 10007). The
        // planner coalesces these into a single REQ on the active
        // account's outbox (NIP-65 write set when known, falling back to
        // `bootstrap_indexer_relays` while the kind:10002 round-trip is
        // pending — same lane the per-kind one-shots used to land on).
        //
        // `limit: None` is intentional: the relay returns the newest
        // replaceable instance per (author, kind) and then tails for
        // future replacements. A capped `limit` would silently truncate
        // mid-session updates if more than one user device republished
        // the same kind in a single tick.
        //
        // `is_indexer_discovery: true` so the cold-start author-unknown
        // arm still lands — the active account's NIP-65 mailbox is
        // unknown until the kind:10002 itself comes back, the canonical
        // bootstrap chicken-and-egg.
        let tailing_self_kinds = selected_self_kinds
            .iter()
            .copied()
            .filter(|kind| !SELF_KINDS_ONESHOT.contains(kind))
            .collect::<BTreeSet<_>>();
        if !tailing_self_kinds.is_empty() {
            self.register_tailing_self_kinds_interest(owner, self_pk.clone(), tailing_self_kinds);
        }

        // The two register_interest(Replace) calls above each enqueue an
        // InvalidateCompile trigger when the interest is new or changed — that
        // is sufficient. No extra ViewOpened needed here.

        // Protocol-specific `#p`-addressed subscriptions (NIP-57 receipts,
        // NIP-25 reactions addressed to the user, …) USED to be emitted here
        // as an M1 REQ on `RelayRole::Content`. D0 forbids the kernel
        // knowing about protocol nouns; those subscriptions are now pushed
        // by host-side runtime controllers as generic
        // `LogicalInterest`s — see the NIP-crate-specific interest helpers
        // (e.g. `nmp_nip57`) and the host-shell controllers (e.g. the
        // external Chirp repo's `nmp-app-chirp/src/zap_receipts_runtime.rs`). The
        // planner's cold-start fallback at
        // `planner/compiler/partition/mod.rs` keeps such interests flowing
        // during the brief window before the active account's kind:10002
        // lands (Tailing + Global + Nip65ReadRelays + #p →
        // bootstrap_content_relays).
        //
        // M2 migration: the self account's kind:0 is fetched by the
        // `SELF_KINDS_TAILING` reactive interest registered above — there is no
        // `profile_requests.requested` dedup set to seed any more.
        let _ = self_pk;
        Vec::new()
    }

    /// Register a single `OneShot + Global` discovery-direction interest
    /// scoped to one author + one kind set, with `limit:1`. Uses `set_sub`
    /// (not `ensure_sub`) so an account switch replaces the prior account's
    /// author in the slot rather than leaking it (V-04).
    ///
    /// ADR-0070 R3 — store-first is universal: this interest is served from the
    /// local store on open (the store-serve half) AND refined by its wire REQ
    /// (the network half). The two halves run together. `is_indexer_discovery`
    /// drives only the wire half's cold-start author-unknown fallback (landing
    /// the REQ on `bootstrap_indexer_relays` when the author's outbox is not yet
    /// known); it does not gate the store-serve, which runs for every interest.
    /// Serving the last-known stored copy immediately is exactly what makes the
    /// app offline-first; the REQ revalidates in place.
    ///
    /// `seed` is the stable, human-readable [`SubKey`] discriminator (e.g.
    /// `"bootstrap:self-dm-relays"`). The matching `InterestId` is derived
    /// from the same seed via `SubKey::new`, so re-mounting the same logical
    /// interest produces the same id — the registry's dedup invariant.
    fn register_oneshot_discovery_interest(
        &mut self,
        owner: SubOwnerKey,
        seed: &'static str,
        kinds: BTreeSet<u32>,
        author: String,
    ) {
        let sub_key = SubKey::new(seed);
        let identity = SubIdentity::new(owner, sub_key, SubScope::Global);
        let shape = InterestShape {
            authors: [author].into_iter().collect(),
            kinds,
            limit: Some(1),
            ..Default::default()
        };
        let interest = LogicalInterest {
            id: InterestId(sub_key.0),
            scope: InterestScope::Global,
            shape,
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: true,
        };
        // Unified front-door: Replace so an account switch swaps the author
        // in-place (store-serve + recompile trigger both fire when changed).
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::Replace,
            }],
            "bootstrap-oneshot-discovery",
        );
    }

    /// Register the cold-start reactive tailing subscription over
    /// [`SELF_KINDS_TAILING`] for `author`. Single REQ, no limit, lifetime
    /// = process (planner CLOSEs only on account switch via Replace replacing
    /// the slot's author, or on explicit registry teardown).
    ///
    /// Uses Replace so an account switch swaps the author in-place
    /// rather than leaving the prior account's REQ live.
    ///
    /// ADR-0070 R3 — store-first is universal: cache-served from the local store
    /// on open (same store-serve half as every consumer interest) AND tailed
    /// over the wire for live republications. Serving the active account's own
    /// stored kind:0/3/10002/… immediately is what hydrates the profile, the
    /// follow set (→ the follow-feed → the timeline), and the relay list on cold
    /// start with no network — the tailing REQ revalidates in place.
    fn register_tailing_self_kinds_interest(
        &mut self,
        owner: SubOwnerKey,
        author: String,
        kinds: BTreeSet<u32>,
    ) {
        let sub_key = SubKey::new("bootstrap:self-kinds-tailing");
        let identity = SubIdentity::new(owner, sub_key, SubScope::Global);
        let shape = InterestShape {
            authors: [author].into_iter().collect(),
            kinds,
            // `limit: None` — Tailing lifecycle, want every replacement
            // republication. See module doc on `SELF_KINDS_TAILING`.
            limit: None,
            ..Default::default()
        };
        let interest = LogicalInterest {
            id: InterestId(sub_key.0),
            scope: InterestScope::Global,
            shape,
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            // Cold-start chicken-and-egg: the active account's NIP-65
            // mailbox is unknown until the kind:10002 itself comes back
            // through this subscription. Opt into the
            // `bootstrap_indexer_relays` fallback so the REQ lands
            // somewhere on cold start; the planner re-routes onto the
            // author's write set on the next recompile after the
            // kind:10002 ingests.
            is_indexer_discovery: true,
        };
        // Unified front-door: Replace so an account switch swaps the author
        // in-place (store-serve + recompile trigger both fire when changed).
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::Replace,
            }],
            "bootstrap-self-kinds",
        );
    }

    /// Selected bootstrap self-kind set after applying the host override.
    ///
    /// `None` means the framework default: the reactive tailing account-config
    /// kinds plus the kind:10050 discovery one-shot. `Some` is authoritative, so
    /// apps can opt out of or reshape either lifecycle lane with one slot.
    fn selected_bootstrap_self_kinds(&self) -> BTreeSet<u32> {
        match self.bootstrap_self_kinds_override() {
            Some(override_kinds) => override_kinds.iter().copied().collect(),
            None => SELF_KINDS_TAILING
                .iter()
                .chain(SELF_KINDS_ONESHOT.iter())
                .copied()
                .collect(),
        }
    }
}

// Bootstrap REQ-emission tests live in a sibling file (kept out of this module
// body to hold startup.rs under the 500-LOC hard ceiling) but compile as a
// child `tests` module so they reach the private bootstrap helpers via
// `super::*`.
#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
