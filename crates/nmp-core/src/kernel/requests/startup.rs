//! Cold-start REQ emission: self profile / NIP-65 relay list / NIP-17 DM relay
//! list, and the active account's kind:3 follow list. No hardcoded seed timeline.

use super::super::{Duration, Instant, Kernel, OutboundMessage};
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

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

        // Each bootstrap interest is `OneShot + Global` so it lands on the
        // planner-extension `bootstrap_indexer_relays` fallback lane (PR #365)
        // — same routing the retired M1 `self.req(RelayRole::Indexer, …)`
        // helper used to fan out to `bootstrap_urls_for_role(Indexer)`.
        // `Global` (not `Account(self_pk)`) is the gate constraint
        // `case_a_authors`'s `is_discovery_oneshot` predicate requires; an
        // account-scoped interest would mark the author unroutable instead
        // of falling through to the bootstrap lane.
        //
        // The owner is a single stable `"kernel:bootstrap"` so the four
        // slots share an owner refcount but stay distinct via their
        // distinct `SubKey`s — re-mounting (e.g. on account switch) is
        // idempotent at the registry layer (`ensure_sub` returns `false`
        // on the re-mount and does not clobber the filter).
        let owner = SubOwnerKey::new("kernel:bootstrap");

        self.register_bootstrap_interest(
            owner,
            "bootstrap:profile-target",
            [0u32].into_iter().collect(),
            self_pk.clone(),
        );
        self.register_bootstrap_interest(
            owner,
            "bootstrap:target-relays",
            [10002u32].into_iter().collect(),
            self_pk.clone(),
        );
        self.register_bootstrap_interest(
            owner,
            "bootstrap:self-dm-relays",
            [10050u32].into_iter().collect(),
            self_pk.clone(),
        );
        self.register_bootstrap_interest(
            owner,
            "bootstrap:self-contacts",
            [3u32].into_iter().collect(),
            self_pk.clone(),
        );

        // Coalesced trigger: even though we may have registered up to four
        // interests, the per-tick inbox coalesces to a single recompile pass
        // (D8). Diagnostic `interest_ids` left empty — the compiler walks
        // the full registry, not a filtered subset.
        self.lifecycle
            .enqueue_trigger(CompileTrigger::ViewOpened { interest_ids: Vec::new() });

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

    /// Register a single `OneShot + Global` bootstrap interest scoped to one
    /// author + one kind set, with `limit:1`. Idempotent via `ensure_sub`.
    ///
    /// `seed` is the stable, human-readable [`SubKey`] discriminator (e.g.
    /// `"bootstrap:profile-target"`). The matching `InterestId` is derived
    /// from the same seed via `SubKey::new`, so re-mounting the same logical
    /// interest produces the same id — the registry's dedup invariant.
    fn register_bootstrap_interest(
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
        };
        let _newly = self.lifecycle.registry_mut().ensure_sub(identity, interest);
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
    /// `identity_state::set_user_configured_relay_edit_rows`; bare
    /// `Kernel::new` tests must install them directly, matching
    /// `discovery_tests::install_bootstrap_relays`.
    ///
    /// Also clears the `cfg(test)` default `wss://purplepag.es` indexer relay
    /// so assertions pin discovery REQs to the test bootstrap relay rather
    /// than collapsing onto the indexer fallback path.
    fn install_bootstrap_relays(kernel: &mut Kernel) {
        let lifecycle = kernel.lifecycle_mut();
        lifecycle.set_indexer_relays(vec![]);
        lifecycle
            .set_bootstrap_indexer_relays(vec!["wss://bootstrap-indexer.test/".to_string()]);
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

    /// True iff at least one REQ in `msgs` carries a filter matching the
    /// `kind`-only, author-pinned, `limit:1` bootstrap shape for `pk`.
    fn has_bootstrap_req(msgs: &[OutboundMessage], kind: u32, pk: &str) -> bool {
        req_filters(msgs).iter().any(|filter| {
            filter["kinds"] == serde_json::json!([kind])
                && filter["authors"] == serde_json::json!([pk])
                && filter["limit"] == serde_json::json!(1)
        })
    }

    /// F-02: active-account bootstrap must emit a kind:10050 REQ pinned to
    /// the active account with `limit:1`, alongside the existing kind:0 /
    /// kind:10002 / kind:3 self fetches. Without this, existing users wait
    /// for a publish→ingest round-trip before the NIP-17 DM inbox
    /// subscription can open against their declared DM relays.
    ///
    /// V-04 Stage 2: the bootstrap interests are now registered through
    /// `InterestRegistry::ensure_sub`; the planner compiles them on the next
    /// `drain_lifecycle_outbound` call. The function itself returns an empty
    /// `Vec<OutboundMessage>` (zero-cost no-op for the caller's `extend`).
    #[test]
    fn bootstrap_emits_kind_10050_dm_relay_list_req_for_active_account() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);
        kernel.active_account = Some(ALICE.to_string());

        // The function itself returns empty post-migration — the planner
        // emits the wire frames on the next drain.
        let direct = kernel.active_account_bootstrap_requests();
        assert!(
            direct.is_empty(),
            "V-04 Stage 2: active_account_bootstrap_requests must return \
             Vec::new() — the planner emits the wire frames on the next drain"
        );

        // Drain the lifecycle to get the planner-emitted REQs.
        let msgs = kernel.drain_lifecycle_outbound();
        assert!(
            !msgs.is_empty(),
            "planner must emit wire frames for the four bootstrap interests"
        );

        // (1) kind:10050 REQ pinned to the active account with limit:1.
        assert!(
            has_bootstrap_req(&msgs, 10050, ALICE),
            "bootstrap must emit a kind:10050 REQ pinned to ALICE with \
             limit:1; got REQs: {:#?}",
            req_filters(&msgs)
        );

        // (2) Pre-existing bootstrap REQs still emitted — the F-02 patch is
        // additive, not a replacement. Locks the four-block shape so a future
        // regression that drops kind:0 / kind:10002 / kind:3 in pursuit of the
        // kind:10050 add is caught here.
        assert!(
            has_bootstrap_req(&msgs, 0, ALICE),
            "kind:0 self-profile REQ must still be emitted"
        );
        assert!(
            has_bootstrap_req(&msgs, 10002, ALICE),
            "kind:10002 self NIP-65 REQ must still be emitted"
        );
        assert!(
            has_bootstrap_req(&msgs, 3, ALICE),
            "kind:3 self-contacts REQ must still be emitted"
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

    /// V-04 Stage 2 idempotence: a re-mount (e.g. account switch back to the
    /// same account) must NOT register additional interests in the registry
    /// — `ensure_sub` is idempotent. Pins the registry shape so a regression
    /// that replaces `ensure_sub` with `set_sub` (which would re-trigger a
    /// compile on every re-mount) is caught.
    #[test]
    fn bootstrap_is_idempotent_under_remount() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        install_bootstrap_relays(&mut kernel);
        kernel.active_account = Some(ALICE.to_string());

        let _ = kernel.active_account_bootstrap_requests();
        let first_count = kernel.lifecycle_mut().registry_mut().len();
        assert_eq!(
            first_count, 4,
            "four bootstrap interests must be registered (profile / NIP-65 / \
             NIP-17 DM relays / contacts)"
        );

        let _ = kernel.active_account_bootstrap_requests();
        let second_count = kernel.lifecycle_mut().registry_mut().len();
        assert_eq!(
            second_count, first_count,
            "re-mount must not register additional interests — ensure_sub is \
             idempotent at the registry layer"
        );
    }
}
