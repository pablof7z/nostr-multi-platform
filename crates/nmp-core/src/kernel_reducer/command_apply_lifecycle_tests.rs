//! #2948 — OneShot read-demand lifecycle threads end-to-end to the wire.
//!
//! Split out of `command_apply_tests.rs` (file-size ceiling, AGENTS.md). These
//! drive the read-demand lifecycle field through the real `KernelReducer` +
//! EOSE ingest path and prove it reaches the wire-sub keep-live decision (the
//! kernel/wire path already implemented both semantics; #2948 only added the
//! ability for the delivery path to *say* which one).
//!
//! Include guard: this file is `#[path]`-included by `kernel_reducer.rs`.

use super::*;
use crate::actor::{ActorCommand, InterestsCommand};
use crate::planner::{InterestLifecycle, InterestShape};
use crate::ObservedProjectionId;
use nmp_network::role::RelayRole;

const RELAY: &str = "wss://relay.example";

/// Collect `(sub_id, filter_json)` for every `["REQ", …]` frame in `frames`.
fn req_subs(frames: &[crate::relay::OutboundMessage]) -> Vec<(String, String)> {
    frames
        .iter()
        .filter_map(|m| {
            let v: serde_json::Value = serde_json::from_str(&m.text).ok()?;
            let arr = v.as_array()?;
            if arr.first()?.as_str()? != "REQ" {
                return None;
            }
            let sub_id = arr.get(1)?.as_str()?.to_string();
            let filter = arr.get(2)?.to_string();
            Some((sub_id, filter))
        })
        .collect()
}

/// Apply an `OpenObservedInterest` with the given lifecycle and return the
/// emitted outbound frames. Mirrors the read-session delivery path: a concept's
/// `ReadDemand.lifecycle` reaches the kernel through this command.
fn open_observed(
    r: &mut KernelReducer,
    filter_json: &str,
    consumer_id: &str,
    lifecycle: InterestLifecycle,
    observer_id: ObservedProjectionId,
) -> Vec<crate::relay::OutboundMessage> {
    let shape = InterestShape::from_filter_json(filter_json).expect("valid filter");
    match r.apply_actor_command(ActorCommand::Interests(
        InterestsCommand::OpenObservedInterest {
            filter_json: filter_json.to_string(),
            consumer_id: consumer_id.to_string(),
            scope: 1,
            relay_pin: None,
            is_indexer_discovery: false,
            lifecycle,
            observer_id,
            replay_shapes: vec![shape],
            replay_limit: 64,
        },
    )) {
        CommandApplyOutcome::Applied(frames) => frames,
        other => panic!("expected Applied, got {other:?}"),
    }
}

/// Deliver an `["EOSE", sub_id]` frame for `relay_url` through the real ingest
/// path (which runs the keep-live / CLOSE-on-EOSE decision).
fn feed_eose(r: &mut KernelReducer, relay_url: &str, sub_id: &str) {
    let eose = serde_json::json!(["EOSE", sub_id]).to_string();
    r.kernel.handle_message(
        RelayRole::Content,
        relay_url,
        crate::kernel::RelayFrame::Text(eose),
    );
}

/// A `OneShot` read demand's compiled REQ must be CLOSEd + evicted at EOSE,
/// while a `Tailing` demand's REQ survives EOSE — proving the lifecycle field
/// threads from the read-demand command all the way to the wire-sub keep-live
/// decision.
#[test]
fn oneshot_observed_read_demand_closes_on_eose_while_tailing_survives() {
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    let oneshot_frames = open_observed(
        &mut r,
        r#"{"kinds":[30402]}"#,
        "ad-collection",
        InterestLifecycle::OneShot,
        ObservedProjectionId(1),
    );
    let oneshot_sub = req_subs(&oneshot_frames)
        .into_iter()
        .find(|(_, filter)| filter.contains("30402"))
        .map(|(sub, _)| sub)
        .expect("OneShot read demand emitted a REQ");

    let tailing_frames = open_observed(
        &mut r,
        r#"{"kinds":[1]}"#,
        "home-feed",
        InterestLifecycle::Tailing,
        ObservedProjectionId(2),
    );
    let tailing_sub = req_subs(&tailing_frames)
        .into_iter()
        .find(|(_, filter)| filter.contains("\"kinds\":[1]"))
        .map(|(sub, _)| sub)
        .expect("Tailing read demand emitted a REQ");

    assert_ne!(
        oneshot_sub, tailing_sub,
        "distinct filters must produce distinct wire sub ids"
    );

    feed_eose(&mut r, RELAY, &oneshot_sub);
    feed_eose(&mut r, RELAY, &tailing_sub);

    let active = r.kernel.snapshot_active_wire_subs();
    assert!(
        !active.iter().any(|(sid, _)| sid == &oneshot_sub),
        "a OneShot read demand must be CLOSEd + evicted at EOSE; active subs: {active:?}"
    );
    assert!(
        active.iter().any(|(sid, _)| sid == &tailing_sub),
        "a Tailing read demand must stay live after EOSE; active subs: {active:?}"
    );
}

/// Checkpoint (#2948): a plan recompile (here: opening another interest — the
/// same class of trigger as a relay reconnect or follow-list change) must NOT
/// re-emit a completed OneShot demand's REQ. EOSE closes the wire sub at the
/// kernel layer, but the planner's `current_plan` still carries the interest,
/// so the plan diff produces no re-REQ — the read stays closed.
#[test]
fn recompile_does_not_reemit_a_completed_oneshot_read_demand() {
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    let oneshot_frames = open_observed(
        &mut r,
        r#"{"kinds":[30402]}"#,
        "ad-collection",
        InterestLifecycle::OneShot,
        ObservedProjectionId(1),
    );
    let oneshot_sub = req_subs(&oneshot_frames)
        .into_iter()
        .find(|(_, filter)| filter.contains("30402"))
        .map(|(sub, _)| sub)
        .expect("OneShot read demand emitted a REQ");

    // Complete the OneShot: EOSE CLOSEs + evicts its wire sub.
    feed_eose(&mut r, RELAY, &oneshot_sub);
    assert!(
        !r
            .kernel
            .snapshot_active_wire_subs()
            .iter()
            .any(|(sid, _)| sid == &oneshot_sub),
        "precondition: the OneShot wire sub is evicted at EOSE"
    );

    // Force a recompile by opening an unrelated interest.
    let recompile_frames = open_observed(
        &mut r,
        r#"{"kinds":[7]}"#,
        "reactions",
        InterestLifecycle::Tailing,
        ObservedProjectionId(2),
    );

    assert!(
        !recompile_frames.iter().any(|m| m.text.contains(&oneshot_sub)),
        "a plan recompile must not re-emit a completed OneShot demand's REQ; got: {:?}",
        recompile_frames.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
    assert!(
        !r
            .kernel
            .snapshot_active_wire_subs()
            .iter()
            .any(|(sid, _)| sid == &oneshot_sub),
        "the completed OneShot must remain closed after the recompile"
    );
}
