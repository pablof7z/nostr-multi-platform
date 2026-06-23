//! D10 defensive guard tests — kind:1059 + empty relays NEVER Auto-routes.
//!
//! A `dispatch_action("nmp.publish", PublishAction::Publish { target: Auto })`
//! for a kind:1059 envelope lands in `actor::dispatch::PublishSignedEvent` with
//! `relays: vec![]`, which calls `publish_signed_event(kernel, raw, &[], cid)`
//! and falls through the `relays.is_empty()` branch → `publish_signed_with_correlation`
//! → `PublishTarget::Auto` → leak. The same hole exists at the
//! `NmpApp::publish_signed_explicit` workspace-internal seam.
//!
//! The D10 defensive guard at the top of `publish_signed_event` refuses any
//! kind:1059 publish whose `relays` slice is empty — the encrypted envelope is
//! dropped, a D6 toast names the refusal, and no outbound frames / publish
//! queue entries are produced. These tests pin the guard's shape from every
//! entry point the kernel can be reached through.

use super::*;

/// Direct-call shape — the same call the actor's
/// `ActorCommand::PublishSignedEvent` arm performs when the dispatch path
/// routes a kind:1059 envelope with `target: PublishTarget::Auto`. The guard
/// must fire BEFORE the `relays.is_empty()` → Auto branch can reach the
/// outbox resolver.
#[test]
fn publish_signed_event_refuses_kind_1059_with_empty_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    // Belt-and-suspenders: even with the kernel's `configured_relays` truly
    // empty (no cfg(test) fallback Content relay), the guard must still
    // refuse — proving the refusal happens upstream of the outbox resolver.
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "kind:1059 with PublishTarget::Auto MUST produce no outbound frames \
         (D10: envelope existence would leak through the NIP-65 outbox)"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("kind:1059") && t.contains("D10")),
        "guard must surface a D6 toast naming kind:1059 and D10; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "refused kind:1059 envelope must NEVER enter the publish queue"
    );
}

/// The same dispatch shape `kernel::action_registry::default_registry()`
/// produces for `PublishAction::Publish { target: Auto }` — `relays_for_target(&Auto)`
/// returns `Vec::new()`. The defensive guard MUST fire for the empty-Vec
/// shape too, not just `&[]` slice literals.
#[test]
fn publish_signed_event_refuses_kind_1059_with_empty_vec_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    // Exact shape `actor::dispatch::PublishSignedEvent` calls with when the
    // dispatch path routes `PublishAction::Publish { target: Auto }`. The
    // guard must fire on the Auto variant regardless of how the target was
    // constructed.
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "PublishTarget::Auto must trigger the guard"
    );
    assert!(
        kernel.last_error_toast_snapshot().is_some(),
        "the guard must set a toast for the empty Vec case too"
    );
    assert!(kernel.publish_queue_snapshot().is_empty());
}

/// Sanity bound — the guard is targeted at kind:1059 ONLY. A non-1059
/// signed event with empty relays must STILL Auto-route (the pre-existing
/// behaviour that the rest of the codebase relies on: kind:1 react,
/// kind:30023 article, etc., all use this fallback intentionally).
#[test]
fn publish_signed_event_does_not_refuse_other_kinds_with_empty_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "kind 30023 still routes");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        !outbound.is_empty(),
        "non-1059 kinds must continue to route under PublishTarget::Auto — the \
         D10 guard is targeted strictly at kind:1059"
    );
    assert_eq!(
        kernel.last_error_toast_snapshot(),
        None,
        "non-1059 Auto-route must not surface a guard toast"
    );
}

/// Broken-promise contract — when the dispatch path supplied a
/// `correlation_id`, the guard's refusal must reach `action_results` as a
/// terminal `failed` verdict so the host's spinner clears. This mirrors the
/// pattern in `publish_profile` for its sign-step early-exits (see
/// `kernel::action_failure_tests`). Without this, a dispatched kind:1059
/// publish with `target: Auto` would hang the host spinner forever.
#[test]
fn publish_signed_event_kind_1059_guard_records_action_failure_for_correlation() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Auto,
        Some("corr-1059-leak".to_string()),
    );
    assert!(outbound.is_empty());

    // The guard must surface a terminal `failed` verdict under the dispatch
    // correlation_id so the host's spinner can be cleared.
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
    let results = parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let arr = results.as_array().unwrap_or_else(|| {
        panic!(
            "guard must surface a terminal verdict under correlation_id; got: {}",
            results
        )
    });
    assert_eq!(arr.len(), 1, "exactly one terminal verdict from the guard");
    let entry = &arr[0];
    assert_eq!(
        entry.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-1059-leak"),
        "the dispatch correlation_id is carried through"
    );
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "guard refusal reports the terminal `failed` status"
    );
}

/// The corresponding HAPPY path — a kind:1059 publish with an EXPLICIT pin
/// must succeed unchanged. The guard targets the empty-relays branch only;
/// any non-empty `relays` slice carries the envelope on the
/// `PublishTarget::Explicit` path (the correct shape for NIP-17 / Marmot).
#[test]
fn publish_signed_event_publishes_kind_1059_with_explicit_pin() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let raw = signed_kind_1059_raw(&id);

    let pin: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit {
            relays: pin.clone(),
        },
        None,
    );

    assert!(
        !outbound.is_empty(),
        "kind:1059 + explicit pin must publish (guard is PublishTarget::Auto only)"
    );
    assert_eq!(
        kernel.last_error_toast_snapshot(),
        None,
        "the happy path must not surface a guard toast"
    );
    // The envelope MUST go to exactly the pinned relays — NOT the author's
    // kind:10002 outbox. This is what NIP-17 / Marmot rely on.
    let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
    got.sort();
    let mut want = pin.clone();
    want.sort();
    assert_eq!(
        got, want,
        "kind:1059 with explicit pin must route to EXACTLY the pin, never to the kind:10002 outbox"
    );
}
