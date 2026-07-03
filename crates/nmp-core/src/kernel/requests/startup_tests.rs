//! Cold-start bootstrap REQ-emission tests for `super::startup`.
//!
//! Split out of `startup.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md file-size rule); compiled as a child `tests` module
//! via `#[path]` so `super::*` still resolves to the bootstrap helpers.
#![cfg(test)]

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
/// Also clears the `cfg(test)` default indexer relay
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
///    10006, 10007) pinned to the active account with NO `limit` — fresh
///    data flows in as the account republishes any of them. kind:10000
///    (mute list) is owned by `MuteRuntimeController` and excluded here.
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

    // (1) Reactive Tailing self-kinds REQ — kinds [0,3,10002,10006,10007],
    // pinned to ALICE, NO `limit` (no truncation of mid-session updates).
    // kind:10000 excluded — owned by MuteRuntimeController.
    assert!(
        has_filter_for(&msgs, ALICE, SELF_KINDS_TAILING, None),
        "bootstrap must emit a Tailing REQ for kinds {:?} pinned to \
         ALICE with no limit; got REQs: {:#?}",
        SELF_KINDS_TAILING,
        req_filters(&msgs),
    );

    // Regression pin for #1817: kind:10007 (NIP-51 search-relay list) MUST
    // be carried by this bundle. Before the fix it was pushed by a bespoke
    // `SearchRelayRuntimeController` whose interest never reached the wire,
    // so `effective_search_relays()` stayed empty and transparent NIP-50
    // `open_search(UserPreferred)` never fanned out. Routing it through the
    // proven self-kinds tailing path (alongside kind:10006) is the fix.
    assert!(
        SELF_KINDS_TAILING.contains(&10007),
        "kind:10007 search-relay list must ride the self-kinds tailing \
         bundle so its self-fetch routes to the wire"
    );

    // (2) kind:10050 NIP-17 DM relay list one-shot with `limit:1`.
    assert!(
        has_filter_for(&msgs, ALICE, &[10050], Some(1)),
        "bootstrap must emit a kind:10050 REQ pinned to ALICE with \
         limit:1; got REQs: {:#?}",
        req_filters(&msgs),
    );
}

/// #2796 — the host bootstrap self-kind override governs both lifecycle lanes.
/// If an app omits kind:10050 from the override, the kernel must not emit the
/// DM-relay one-shot through a separate hardcoded path.
#[test]
fn bootstrap_self_kind_override_can_opt_out_of_dm_relay_oneshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);
    kernel.set_bootstrap_self_kinds_override(Some(vec![0, 10002]));
    kernel.active_account = Some(ALICE.to_string());

    let _ = kernel.active_account_bootstrap_requests();
    let msgs = kernel.drain_lifecycle_outbound();

    assert!(
        has_filter_for(&msgs, ALICE, &[0, 10002], None),
        "override kinds must still tail as the selected self-kind set; \
         got REQs: {:#?}",
        req_filters(&msgs),
    );
    assert!(
        !some_req_carries_kind_for(&msgs, ALICE, 10050),
        "omitting kind:10050 from the override must opt out of the \
         DM-relay one-shot; got REQs: {:#?}",
        req_filters(&msgs),
    );
}

/// #2796 — including kind:10050 in the override keeps the OneShot semantics,
/// while the remaining selected self-kinds use the Tailing lane.
#[test]
fn bootstrap_self_kind_override_keeps_kind10050_oneshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);
    kernel.set_bootstrap_self_kinds_override(Some(vec![0, 10050, 30078]));
    kernel.active_account = Some(ALICE.to_string());

    let _ = kernel.active_account_bootstrap_requests();
    let msgs = kernel.drain_lifecycle_outbound();

    assert!(
        has_filter_for(&msgs, ALICE, &[10050], Some(1)),
        "kind:10050 from the override must be emitted as a OneShot with \
         limit:1; got REQs: {:#?}",
        req_filters(&msgs),
    );
    assert!(
        has_filter_for(&msgs, ALICE, &[0, 30078], None),
        "non-one-shot override kinds must be emitted on the Tailing lane; \
         got REQs: {:#?}",
        req_filters(&msgs),
    );
    assert!(
        !has_filter_for(&msgs, ALICE, &[0, 10050, 30078], None),
        "kind:10050 must not be folded into the Tailing filter; got REQs: \
         {:#?}",
        req_filters(&msgs),
    );
}

/// True iff at least one REQ in `msgs` is author-pinned to `pk` AND its
/// `kinds` array contains `kind` (membership, not exact-set equality).
fn some_req_carries_kind_for(msgs: &[OutboundMessage], pk: &str, kind: u32) -> bool {
    req_filters(msgs).iter().any(|filter| {
        let author_ok = filter["authors"] == serde_json::json!([pk]);
        let kind_present = filter["kinds"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v.as_u64() == Some(u64::from(kind))))
            .unwrap_or(false);
        author_ok && kind_present
    })
}

/// #1817 regression — transparent NIP-50 search depends on the active
/// account's kind:10007 (NIP-51 search-relay list) self-fetch actually
/// reaching the wire as a compiled/routed REQ. Before the fix the list was
/// pushed by a bespoke `SearchRelayRuntimeController` whose interest never
/// compiled into a REQ, so `effective_search_relays()` stayed empty and
/// `open_search(UserPreferred)` never fanned out. This test pins that
/// kind:10007 is carried by the bootstrap self-kinds REQ, author-pinned to
/// the active account — the wire half of the fix (the projection half is
/// covered by `explicit composition`'s `search_relay_transparency.rs`).
#[test]
fn bootstrap_routes_kind10007_search_relay_self_fetch() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);
    kernel.active_account = Some(ALICE.to_string());

    let _ = kernel.active_account_bootstrap_requests();
    let msgs = kernel.drain_lifecycle_outbound();

    assert!(
        some_req_carries_kind_for(&msgs, ALICE, 10007),
        "the active account's kind:10007 search-relay list self-fetch \
         MUST appear as a compiled/routed REQ pinned to ALICE; got REQs: \
         {:#?}",
        req_filters(&msgs),
    );
}

/// Account-switch must re-target the kind:10007 self-fetch onto the new
/// account and stop carrying the prior account's — matching the kind:10006
/// behaviour. Pins that the search-relay self-fetch does not leak across
/// accounts (a privacy/staleness regression for transparent search).
#[test]
fn account_switch_retargets_kind10007_self_fetch() {
    const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    kernel.active_account = Some(ALICE.to_string());
    let _ = kernel.active_account_bootstrap_requests();
    let _ = kernel.drain_lifecycle_outbound();

    kernel.active_account = Some(BOB.to_string());
    let _ = kernel.active_account_bootstrap_requests();
    let msgs = kernel.drain_lifecycle_outbound();

    assert!(
        some_req_carries_kind_for(&msgs, BOB, 10007),
        "after account switch the kind:10007 self-fetch must be pinned to \
         BOB; got REQs: {:#?}",
        req_filters(&msgs),
    );
    assert!(
        !some_req_carries_kind_for(&msgs, ALICE, 10007),
        "after account switch ALICE's kind:10007 self-fetch must not be \
         re-emitted (no cross-account leak); got REQs: {:#?}",
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
