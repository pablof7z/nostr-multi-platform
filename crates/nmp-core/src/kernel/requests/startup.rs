//! Cold-start REQ emission: self profile / NIP-65 relay list / NIP-17 DM relay
//! list / kind:10000 mute list / kind:10006 blocked-relay list, and the active
//! account's kind:3 follow list. No hardcoded seed timeline.

use super::super::{Duration, Instant, Kernel, OutboundMessage};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Self-fetched account-config kinds the cold-start tailing subscription
/// keeps live after sign-in.
///
/// Reactive design: the host wires up its kind:0 / kind:3 / kind:10002 /
/// kind:10000 / kind:10006 readers exactly once at sign-in and gets fresh
/// data automatically whenever the account republishes any of these. The
/// pre-V-04 model fired a one-shot REQ per kind and closed it on EOSE,
/// which left apps stale after the first round-trip and forced ad-hoc
/// re-fetch loops in each view module.
///
/// - **0**: profile metadata
/// - **3**: contacts (follow list — the timeline depends on this staying
///   fresh as the user mutes / unfollows)
/// - **10002**: NIP-65 relay list (mailboxes — routing decisions must
///   re-resolve when the user edits this from a second device)
/// - **10000**: mute list
/// - **10006**: blocked-relay list (fed into the [`crate::substrate::BlockedRelayLookup`]
///   handle so the router's subtractive blocked-set post-pass picks up
///   changes mid-session)
const SELF_KINDS_TAILING: &[u32] = &[0, 3, 10002, 10000, 10006];

impl Kernel {
    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));
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
    /// V-04 Stage 2: the four bootstrap interests are registered through
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

        // ── Discovery-direction one-shots (kind:10050 only) ───────────────
        //
        // kind:10050 (NIP-17 DM relay list) intentionally stays a OneShot:
        // it is consumed by the DM gift-wrap publish path on demand, the
        // recipient's `dm_inbox_relays` cache is a read-once snapshot, and
        // tailing it would multiply REQ pressure on the indexer for no
        // observable behavioural win. The other config kinds move to
        // tailing below.
        //
        // `is_indexer_discovery: true` opts the interest into
        // `case_a_authors`'s `bootstrap_indexer_relays` fallback so the
        // cold-start author-unknown case lands a REQ instead of falling
        // through to `unroutable`.
        self.register_oneshot_discovery_interest(
            owner,
            "bootstrap:self-dm-relays",
            [10050u32].into_iter().collect(),
            self_pk.clone(),
        );

        // ── Reactive tailing self-kind subscription ──────────────────────
        //
        // One Tailing interest carrying every account-config kind in
        // `SELF_KINDS_TAILING` (kinds 0, 3, 10002, 10000, 10006). The
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
        self.register_tailing_self_kinds_interest(owner, self_pk.clone());

        // Coalesced trigger: per-tick inbox collapses the registrations
        // above into a single recompile pass (D8). Diagnostic
        // `interest_ids` left empty — the compiler walks the full
        // registry, not a filtered subset.
        self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
            interest_ids: Vec::new(),
        });

        // Protocol-specific `#p`-addressed subscriptions (NIP-57 receipts,
        // NIP-25 reactions addressed to the user, …) USED to be emitted here
        // as an M1 REQ on `RelayRole::Content`. D0 forbids the kernel
        // knowing about protocol nouns; those subscriptions are now pushed
        // by host-side runtime controllers as generic
        // `LogicalInterest`s — see the NIP-crate-specific interest helpers
        // (e.g. `nmp_nip57`) and the host-shell controllers (e.g.
        // `apps/chirp/nmp-app-chirp/src/zap_receipts_runtime.rs`). The
        // planner's cold-start fallback at
        // `planner/compiler/partition/mod.rs` keeps such interests flowing
        // during the brief window before the active account's kind:10002
        // lands (Tailing + Global + Nip65ReadRelays + #p →
        // bootstrap_content_relays).
        self.profile_requests.requested.insert(self_pk);
        Vec::new()
    }

    /// Register a single `OneShot + Global` discovery-direction interest
    /// scoped to one author + one kind set, with `limit:1`. Uses `set_sub`
    /// (not `ensure_sub`) so an account switch replaces the prior account's
    /// author in the slot rather than leaking it (V-04).
    ///
    /// ADR-0045 — intentionally does NOT route through
    /// [`crate::kernel::Kernel::enqueue_interest_cache_serve`]: these are
    /// `is_indexer_discovery` bootstrap lanes whose explicit intent is a fresh
    /// network fetch (the cold-start author-unknown fallback). Serving a
    /// possibly-stale store copy would defeat the bootstrap. The store-first
    /// uniformity guarantee applies to consumer interests, not these
    /// discovery-direction bootstrap REQs.
    ///
    /// `seed` is the stable, human-readable [`SubKey`] discriminator (e.g.
    /// `"bootstrap:self-dm-relays"`). The matching `InterestId` is derived
    /// from the same seed via `SubKey::new`, so re-mounting the same logical
    /// interest produces the same id — the registry's dedup invariant.
    fn register_oneshot_discovery_interest(
        &mut self,
        owner: SubOwnerKey,
        seed: &'static str,
        kinds: std::collections::BTreeSet<u32>,
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
        self.lifecycle.registry_mut().set_sub(identity, interest);
    }

    /// Register the cold-start reactive tailing subscription over
    /// [`SELF_KINDS_TAILING`] for `author`. Single REQ, no limit, lifetime
    /// = process (planner CLOSEs only on account switch via `set_sub`
    /// replacing the slot's author, or on explicit registry teardown).
    ///
    /// Uses `set_sub` so an account switch swaps the author in-place
    /// rather than leaving the prior account's REQ live.
    ///
    /// ADR-0045 — intentionally NOT cache-served (same rationale as
    /// `register_oneshot_discovery_interest`): an `is_indexer_discovery`
    /// bootstrap REQ whose intent is the live republication of the active
    /// account's replaceable kinds, not a store replay.
    fn register_tailing_self_kinds_interest(&mut self, owner: SubOwnerKey, author: String) {
        let sub_key = SubKey::new("bootstrap:self-kinds-tailing");
        let identity = SubIdentity::new(owner, sub_key, SubScope::Global);
        // FFI override slot beats the builtin list — apps that need a
        // different replaceable-kind set (e.g. a publish-only app that
        // doesn't care about kind:10006 blocked relays) install one via
        // `nmp_app_set_bootstrap_self_kinds` before `nmp_app_start`.
        let kinds_iter: Box<dyn Iterator<Item = u32>> = match self.bootstrap_self_kinds_override() {
            Some(override_kinds) => Box::new(override_kinds.to_vec().into_iter()),
            None => Box::new(SELF_KINDS_TAILING.iter().copied()),
        };
        let shape = InterestShape {
            authors: [author].into_iter().collect(),
            kinds: kinds_iter.collect(),
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
        self.lifecycle.registry_mut().set_sub(identity, interest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use serde_json::Value;

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// V-04 Stage 2: install the planner-extension bootstrap relay lanes so
    /// the planner has somewhere to land the `OneShot + Global` bootstrap
    /// interests. Production wires these from `bootstrap_urls_for_role` in
    /// `identity_state::set_configured_relays`; bare
    /// `Kernel::new` tests must install them directly, matching
    /// `discovery_tests::install_bootstrap_relays`.
    ///
    /// Also clears the `cfg(test)` default `wss://purplepag.es` indexer relay
    /// so assertions pin discovery REQs to the test bootstrap relay rather
    /// than collapsing onto the indexer fallback path.
    fn install_bootstrap_relays(kernel: &mut Kernel) {
        let lifecycle = kernel.lifecycle_mut();
        lifecycle.set_indexer_relays(vec![]);
        lifecycle.set_bootstrap_indexer_relays(vec!["wss://bootstrap-indexer.test/".to_string()]);
    }

    /// Extract the REQ frames from a list of `OutboundMessage`s. V-04 Stage 2:
    /// sub-ids are now planner-assigned `sub-<hash>` strings, not the
    /// human-readable `"profile-target"` / `"self-dm-relays"` / … labels —
    /// so assertions must grep on filter content (kinds / authors / limit)
    /// inside `text`, not on sub-id substrings.
    fn req_filters(msgs: &[OutboundMessage]) -> Vec<Value> {
        msgs.iter()
            .filter_map(|m| {
                let parsed: Value = serde_json::from_str(&m.text).ok()?;
                let arr = parsed.as_array()?;
                if arr.first()? != "REQ" {
                    return None;
                }
                arr.get(2).cloned()
            })
            .collect()
    }

    /// True iff at least one REQ in `msgs` carries a filter author-pinned
    /// to `pk` whose `kinds` array equals `expected_kinds` (order-insensitive)
    /// and whose `limit` matches `expected_limit` (`None` = no `limit` key).
    fn has_filter_for(
        msgs: &[OutboundMessage],
        pk: &str,
        expected_kinds: &[u32],
        expected_limit: Option<u32>,
    ) -> bool {
        let want_kinds: std::collections::BTreeSet<u32> = expected_kinds.iter().copied().collect();
        req_filters(msgs).iter().any(|filter| {
            let author_ok = filter["authors"] == serde_json::json!([pk]);
            let kinds_ok = filter["kinds"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u32))
                        .collect::<std::collections::BTreeSet<u32>>()
                })
                .map_or(false, |k| k == want_kinds);
            let limit_ok = match expected_limit {
                Some(n) => filter["limit"] == serde_json::json!(n),
                None => filter.get("limit").is_none() || filter["limit"].is_null(),
            };
            author_ok && kinds_ok && limit_ok
        })
    }

    /// Active-account bootstrap must emit:
    /// 1. One reactive Tailing REQ for the self-kinds (kinds 0, 3, 10002,
    ///    10000, 10006) pinned to the active account with NO `limit` —
    ///    fresh data flows in as the account republishes any of them.
    /// 2. A kind:10050 OneShot pinned to the active account with `limit:1`
    ///    (NIP-17 DM relay list — F-02 cold-start fetch, intentionally
    ///    NOT folded into the tailing REQ because the DM gift-wrap
    ///    publish path reads it on-demand, not reactively).
    ///
    /// V-04 Stage 2: the bootstrap interests are registered through the
    /// `InterestRegistry`; the planner compiles them on the next
    /// `drain_lifecycle_outbound` call. The function itself returns an
    /// empty `Vec<OutboundMessage>` (zero-cost no-op for the caller's
    /// `extend`).
    #[test]
    fn bootstrap_emits_tailing_self_kinds_plus_dm_relay_oneshot() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);
        kernel.active_account = Some(ALICE.to_string());

        let direct = kernel.active_account_bootstrap_requests();
        assert!(
            direct.is_empty(),
            "active_account_bootstrap_requests must return Vec::new() — \
             the planner emits the wire frames on the next drain"
        );

        let msgs = kernel.drain_lifecycle_outbound();
        assert!(!msgs.is_empty(), "planner must emit bootstrap wire frames");

        // (1) Reactive Tailing self-kinds REQ — kinds [0,3,10002,10000,10006],
        // pinned to ALICE, NO `limit` (no truncation of mid-session updates).
        assert!(
            has_filter_for(&msgs, ALICE, SELF_KINDS_TAILING, None),
            "bootstrap must emit a Tailing REQ for kinds {:?} pinned to \
             ALICE with no limit; got REQs: {:#?}",
            SELF_KINDS_TAILING,
            req_filters(&msgs),
        );

        // (2) kind:10050 NIP-17 DM relay list one-shot with `limit:1`.
        assert!(
            has_filter_for(&msgs, ALICE, &[10050], Some(1)),
            "bootstrap must emit a kind:10050 REQ pinned to ALICE with \
             limit:1; got REQs: {:#?}",
            req_filters(&msgs),
        );
    }

    /// Without an active account, bootstrap is a no-op — the existing
    /// contract (early return on `None`) must continue to hold, including
    /// for the new kind:10050 path. Pins the negative case so a future
    /// "always fetch" refactor that ignores `active_account` is caught.
    ///
    /// V-04 Stage 2: the contract now means "no `ensure_sub` calls and no
    /// trigger enqueued" → the planner has nothing to compile → the next
    /// `drain_lifecycle_outbound` returns empty.
    #[test]
    fn bootstrap_emits_no_dm_relay_list_req_without_active_account() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);
        kernel.active_account = None;

        let direct = kernel.active_account_bootstrap_requests();
        assert!(direct.is_empty(), "early-return path returns empty");

        let msgs = kernel.drain_lifecycle_outbound();
        assert!(
            msgs.is_empty(),
            "no active account → no bootstrap interests registered → \
             planner emits no wire frames; got: {:#?}",
            msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
        );
    }

    /// Re-mount must not register additional `(scope, key)` slots in the
    /// registry. The bootstrap path uses `set_sub` (NOT `ensure_sub`) so the
    /// slot's author cell is replaced in-place across re-mounts / account
    /// switches; the SLOT COUNT stays at exactly two (one Tailing self-kinds
    /// slot + one OneShot kind:10050 slot). Pins the registry-shape
    /// invariant so a regression that mints fresh slots per call (e.g.
    /// account-pubkey-derived `SubKey`s) is caught.
    #[test]
    fn bootstrap_is_idempotent_under_remount() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);
        kernel.active_account = Some(ALICE.to_string());

        let _ = kernel.active_account_bootstrap_requests();
        let first_count = kernel.lifecycle_mut().registry_mut().len();
        assert_eq!(
            first_count, 2,
            "two bootstrap slots must be registered (Tailing self-kinds + \
             OneShot kind:10050)"
        );

        let _ = kernel.active_account_bootstrap_requests();
        let second_count = kernel.lifecycle_mut().registry_mut().len();
        assert_eq!(
            second_count, first_count,
            "re-mount must not register additional slots — `set_sub` \
             replaces in-place"
        );
    }

    /// Account-switch eviction: bootstrapping under a different
    /// `active_account` must replace the prior account's author in the
    /// slot, not leak it across the switch. This is the V-04 stale-feed
    /// fix that motivated moving from `ensure_sub` to `set_sub`.
    #[test]
    fn account_switch_replaces_self_kinds_author_in_slot() {
        const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);

        // Sign in as ALICE — slot carries ALICE.
        kernel.active_account = Some(ALICE.to_string());
        let _ = kernel.active_account_bootstrap_requests();
        let _drained_alice = kernel.drain_lifecycle_outbound();

        // Switch to BOB. The slot count must stay at 2 (set_sub replaces in
        // place); the author in the Tailing self-kinds REQ must be BOB.
        kernel.active_account = Some(BOB.to_string());
        let _ = kernel.active_account_bootstrap_requests();
        assert_eq!(
            kernel.lifecycle_mut().registry_mut().len(),
            2,
            "account switch must NOT mint additional registry slots"
        );

        let msgs = kernel.drain_lifecycle_outbound();
        assert!(
            has_filter_for(&msgs, BOB, SELF_KINDS_TAILING, None),
            "after account switch, the Tailing self-kinds REQ must be \
             pinned to BOB (not ALICE); got REQs: {:#?}",
            req_filters(&msgs)
        );
        // ALICE must no longer appear as an author in any newly-emitted
        // bootstrap REQ — her slot was replaced, not duplicated.
        assert!(
            !has_filter_for(&msgs, ALICE, SELF_KINDS_TAILING, None),
            "after account switch, ALICE must NOT still be subscribed to her \
             own self-kinds (stale-feed leak); got REQs: {:#?}",
            req_filters(&msgs)
        );
    }
}
