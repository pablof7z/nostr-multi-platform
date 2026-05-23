//! T170 RED — relay-scoped `wire_subs` / `persistent_subs` keying.
//!
//! Bug class #161/#166: the M2 planner deliberately reuses the same `sub-*`
//! id across multiple relay URLs for one filter (per NIP-01 §1 sub ids are
//! per-connection — see `subs/wire.rs` "Sub-id stability"). The kernel's
//! `wire_subs` and `persistent_subs` were keyed by `sub_id` ALONE, so two
//! relays carrying the same follow-feed filter collide:
//!
//! - the second `WireFrame::Req` clobbers the first relay's `wire_subs` row;
//! - a `WireFrame::Close` for ONE relay removes the single shared row and
//!   `unregister_persistent_sub`s the sub — so the still-live SIBLING relay
//!   loses its persistence and auto-CLOSEs on its next EOSE.
//!
//! That is a degraded re-emergence of the exact bug T140-FF fixed (the
//! follow-feed dies after first EOSE). The fix keys both maps by
//! `(relay_url, sub_id)`, matching the `plan_diff` precedent (#161).
//!
//! These tests MUST FAIL before the keying fix and MUST PASS after.

use super::*;
use crate::planner::{InterestId, InterestLifecycle};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

// Canonical URL forms (no empty-path trailing slash). The kernel keys
// `wire_subs` / `persistent_subs` by the canonical URL — the planner boundary
// and the EOSE handler both canonicalize (T-relay-url-normalize) — so the
// relay-scoped-keying assertions below operate on the canonical form.
const RELAY_A: &str = "wss://relay-a.t170";
const RELAY_B: &str = "wss://relay-b.t170";
const SHARED_SUB: &str = "sub-t170shared";

/// Two relays serve the SAME follow-feed filter (same `sub_id`, Tailing).
/// After a CLOSE for relay A, relay B's `wire_subs` row must survive.
///
/// Pre-fix: sub_id-only keying — relay B's REQ clobbered relay A's row, then
/// the CLOSE removed the single shared row → snapshot empty → FAILS.
/// Post-fix: `(relay_url, sub_id)` keying — relay B's row is independent and
/// survives the relay-A CLOSE → PASSES.
#[test]
fn t170_sibling_relay_wire_sub_row_survives_close_of_other_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let req = |relay_url: &str| WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: SHARED_SUB.to_string(),
        filter_json: r#"{"kinds":[1,6],"authors":["aa"],"limit":200}"#.to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::Tailing,
    };

    // Both relays open the shared follow-feed sub.
    kernel.register_wire_frames_for_test(&[req(RELAY_A), req(RELAY_B)]);

    // The planner withdraws relay A only (e.g. NIP-65 re-route drops relay A
    // but relay B still carries the follow). CLOSE travels for relay A.
    kernel.register_wire_frames_for_test(&[WireFrame::Close {
        relay_url: RELAY_A.to_string(),
        sub_id: SHARED_SUB.to_string(),
    }]);

    let active = kernel.snapshot_active_wire_subs();
    assert!(
        active
            .iter()
            .any(|(sid, url)| sid == SHARED_SUB && url == RELAY_B),
        "T170: relay B's wire_subs row for the shared follow-feed sub must \
         survive a CLOSE issued for relay A; got active subs: {active:?}"
    );
    assert!(
        !active
            .iter()
            .any(|(sid, url)| sid == SHARED_SUB && url == RELAY_A),
        "T170: relay A's row must be gone after its CLOSE; got: {active:?}"
    );
}

/// Sibling-relay persistence must survive a CLOSE for the other relay.
///
/// Behavioral proof of the degraded re-emergence: after CLOSE for relay A,
/// relay B answers EOSE. A `Tailing` follow-feed sub must stay `live` (the
/// T140-FF keep-live contract). If the relay-A CLOSE clobbered the shared
/// persistence registration, relay B's EOSE auto-CLOSEs the sub → state is
/// NOT `live` → FAILS (pre-fix). Post-fix relay B's persistence is
/// independent → stays `live` → PASSES.
#[test]
fn t170_sibling_relay_persistence_survives_close_of_other_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let req = |relay_url: &str| WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: SHARED_SUB.to_string(),
        filter_json: r#"{"kinds":[1,6],"authors":["aa"],"limit":200}"#.to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::Tailing,
    };

    kernel.register_wire_frames_for_test(&[req(RELAY_A), req(RELAY_B)]);
    kernel.register_wire_frames_for_test(&[WireFrame::Close {
        relay_url: RELAY_A.to_string(),
        sub_id: SHARED_SUB.to_string(),
    }]);

    // Relay B answers EOSE for the shared sub.
    let eose = serde_json::json!(["EOSE", SHARED_SUB]).to_string();
    kernel.handle_message(
        crate::relay::RelayRole::Content,
        RELAY_B,
        RelayFrame::Text(eose),
    );

    let state = kernel.wire_sub_state_for_test_on_relay(RELAY_B, SHARED_SUB);
    assert_eq!(
        state.as_deref(),
        Some("live"),
        "T170: relay B's Tailing follow-feed sub must stay `live` after EOSE \
         even though relay A was CLOSEd (persistence must be relay-scoped, \
         not clobbered by the sibling's CLOSE); got state {state:?}"
    );
}

/// T-relay-url-normalize — planner URL canonicalization at the wire-sub boundary.
///
/// Bug class: planner-emitted `relay_url`s originate from kind:10002 NIP-65
/// relay lists — arbitrary, user-typed strings (mixed case, empty-path
/// trailing slash). The transport pool keys every socket — and every
/// `RelayEvent` a worker emits — on the *canonical* URL, so the EOSE handler
/// looks up `wire_subs` / `persistent_subs` under the canonical delivering URL.
///
/// Pre-fix: `register_planner_wire_frames` keyed both maps by the RAW planner
/// URL. A `Tailing` follow-feed sub registered under `wss://Relay.MIXED/` was
/// never found by `is_persistent_sub("wss://relay.mixed", sub_id)` — the EOSE
/// handler wrongly auto-CLOSEd the follow feed and the stale row leaked.
/// Post-fix: the boundary canonicalizes, so registration and the EOSE lookup
/// agree → the sub stays `live`.
#[test]
fn t_normalize_planner_url_persistent_sub_survives_eose_on_canonical_url() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Planner emits a non-canonical URL (uppercase host + empty-path trailing
    // slash), exactly as a kind:10002 relay list might carry it.
    const RAW_URL: &str = "wss://Relay.MIXED/";
    // The transport pool canonicalizes; every RelayEvent (incl. EOSE) the
    // worker stamps carries this form.
    const CANONICAL_URL: &str = "wss://relay.mixed";
    const SUB: &str = "sub-t-normalize";

    kernel.register_wire_frames_for_test(&[WireFrame::Req {
        relay_url: RAW_URL.to_string(),
        sub_id: SUB.to_string(),
        filter_json: r#"{"kinds":[1,6],"authors":["aa"],"limit":200}"#.to_string(),
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::Tailing,
    }]);

    // The wire-sub row must be keyed under the canonical URL so the EOSE
    // handler (which uses the transport-stamped canonical URL) can find it.
    assert_eq!(
        kernel
            .wire_sub_state_for_test_on_relay(CANONICAL_URL, SUB)
            .as_deref(),
        Some("opening"),
        "T-relay-url-normalize: planner Req must register the wire-sub row \
         under the CANONICAL relay URL, not the raw kind:10002 form"
    );

    // EOSE arrives on the canonical URL (as the transport always delivers it).
    let eose = serde_json::json!(["EOSE", SUB]).to_string();
    kernel.handle_message(
        crate::relay::RelayRole::Content,
        CANONICAL_URL,
        RelayFrame::Text(eose),
    );

    // The Tailing follow-feed sub must stay `live` — the persistent-sub
    // registration was canonicalized so `is_persistent_sub` matches.
    assert_eq!(
        kernel
            .wire_sub_state_for_test_on_relay(CANONICAL_URL, SUB)
            .as_deref(),
        Some("live"),
        "T-relay-url-normalize: a Tailing follow-feed sub registered with a \
         non-canonical planner URL must survive EOSE on the canonical URL — \
         persistent-sub keying must canonicalize at the planner boundary"
    );
}

/// T-relay-url-normalize — a `WireFrame::Close` with a non-canonical URL must
/// still evict the row registered under the canonical key.
///
/// Pre-fix: a Close emitted with the raw kind:10002 URL form removed nothing
/// (the row lived under the canonical key) — the sub leaked and stayed pinned
/// in `persistent_subs`. Post-fix: the Close arm canonicalizes too, so the
/// row and its persistence registration are torn down.
#[test]
fn t_normalize_planner_close_with_non_canonical_url_evicts_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    const RAW_URL: &str = "wss://Relay.MIXED/";
    const CANONICAL_URL: &str = "wss://relay.mixed";
    const SUB: &str = "sub-t-normalize-close";

    kernel.register_wire_frames_for_test(&[WireFrame::Req {
        relay_url: RAW_URL.to_string(),
        sub_id: SUB.to_string(),
        filter_json: r#"{"kinds":[1],"authors":["aa"],"limit":10}"#.to_string(),
        interest_id: InterestId(2),
        lifecycle: InterestLifecycle::Tailing,
    }]);
    assert!(
        kernel
            .wire_sub_state_for_test_on_relay(CANONICAL_URL, SUB)
            .is_some(),
        "precondition: row registered under canonical URL"
    );

    // Close emitted with a DIFFERENT non-canonical spelling of the same URL.
    kernel.register_wire_frames_for_test(&[WireFrame::Close {
        relay_url: "WSS://relay.mixed/".to_string(),
        sub_id: SUB.to_string(),
    }]);

    assert!(
        kernel
            .wire_sub_state_for_test_on_relay(CANONICAL_URL, SUB)
            .is_none(),
        "T-relay-url-normalize: a Close with a non-canonical URL must evict \
         the wire-sub row keyed under the canonical URL"
    );
    let active = kernel.snapshot_active_wire_subs();
    assert!(
        !active.iter().any(|(sid, _url)| sid == SUB),
        "T-relay-url-normalize: the sub must not remain active after a \
         non-canonical Close; got active subs: {active:?}"
    );
}

/// T-relay-url-normalize — the `*_persistent_sub` primitives canonicalize their
/// `relay_url` argument internally, so a caller that registers with a
/// non-canonical URL (e.g. the NWC wallet path, whose `NwcUri` relay is NOT
/// canonicalized) is still matched by the EOSE handler's canonical lookup.
///
/// Pre-fix: `register_persistent_sub` keyed the set by the raw argument; a NWC
/// kind:23195 listener registered under `wss://Wallet.RELAY/` was never found
/// by `is_persistent_sub("wss://wallet.relay", …)` — the listener would be
/// wrongly auto-CLOSE'd on its first EOSE. Post-fix the primitive canonicalizes.
#[test]
fn t_normalize_persistent_sub_primitive_canonicalizes_relay_url() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    const RAW_URL: &str = "wss://Wallet.RELAY/";
    const CANONICAL_URL: &str = "wss://wallet.relay";
    const SUB: &str = "nwc-deadbeef";

    // Register with the raw (non-canonical) URL form, exactly as the NWC
    // wallet path does via `NwcUri::primary_relay_url()`.
    kernel.register_persistent_sub(RAW_URL, SUB);

    // The EOSE handler looks up by the canonical delivering URL — it must hit.
    assert!(
        kernel.is_persistent_sub(CANONICAL_URL, SUB),
        "T-relay-url-normalize: a persistent sub registered with a \
         non-canonical URL must be found by a canonical-URL lookup"
    );
    // A different non-canonical spelling resolves to the same key too.
    assert!(
        kernel.is_persistent_sub("WSS://wallet.relay", SUB),
        "T-relay-url-normalize: any URL spelling must resolve to the same \
         persistent-sub key"
    );

    // Unregister with yet another spelling — the canonical key must be removed.
    kernel.unregister_persistent_sub("wss://wallet.relay/", SUB);
    assert!(
        !kernel.is_persistent_sub(CANONICAL_URL, SUB),
        "T-relay-url-normalize: unregister with a non-canonical URL must \
         remove the canonical-keyed persistent-sub entry"
    );
}
