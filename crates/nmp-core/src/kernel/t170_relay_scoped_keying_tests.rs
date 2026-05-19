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
//! `(relay_url, sub_id)`, matching the `plan_diff` precedent (#161) and the
//! `LifecycleGate.known_subs` precedent (#166).
//!
//! These tests MUST FAIL before the keying fix and MUST PASS after.

use super::*;
use crate::planner::{InterestId, InterestLifecycle};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const RELAY_A: &str = "wss://relay-a.t170/";
const RELAY_B: &str = "wss://relay-b.t170/";
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
        Message::Text(eose),
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
