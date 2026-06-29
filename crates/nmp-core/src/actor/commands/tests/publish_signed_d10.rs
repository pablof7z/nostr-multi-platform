//! Pre-signed target + D10 guard tests.
//!
//! Externally signed publish is no longer a normal app write path. It is an
//! internal/protocol/import escape that must carry an explicit non-empty
//! provenance-tagged route. `Auto`, `ManualOverride`, and `Diagnostic` fail
//! before any signed event is published. D10 remains stricter for private
//! envelope kinds: kind:1059/14 still require `VerifiedPrivateInbox`.

use super::*;

#[test]
fn publish_signed_event_refuses_kind_1059_with_auto_target() {
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
        "kind:1059 with PublishTarget::Auto MUST produce no outbound frames"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("pre-signed publish target rejected")),
        "guard must surface a pre-signed target rejection toast; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        kernel.publish_queue_snapshot().is_empty(),
        "refused kind:1059 envelope must NEVER enter the publish queue"
    );
}

#[test]
fn publish_signed_event_refuses_kind_1059_with_empty_vec_relays() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    kernel.clear_configured_relays_for_test();
    let raw = signed_kind_1059_raw(&id);

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

#[test]
fn publish_signed_event_refuses_other_kinds_with_auto_target() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "kind 30023 still routes");

    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();
    let outbound = publish_signed_event(&mut kernel, raw, PublishTarget::Auto, None);

    assert!(
        outbound.is_empty(),
        "all pre-signed publishes must reject PublishTarget::Auto"
    );
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("pre-signed publish target rejected")),
        "Auto pre-signed publish must surface a target rejection toast"
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

#[test]
fn publish_signed_event_rejects_manual_override_for_presigned() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "manual override rejected");
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();

    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::manual_override(vec!["wss://manual.example".to_string()]),
        None,
    );

    assert!(outbound.is_empty());
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("manual_override")),
        "manual_override rejection should be visible; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

#[test]
fn publish_signed_event_rejects_diagnostic_for_presigned() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let (json, _ev_id, _sig) = signed_nip01_json(&id, "diagnostic rejected");
    let raw: crate::store::RawEvent = serde_json::from_str(&json).unwrap();

    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            vec!["wss://diagnostic.example".to_string()],
            PublishRouteClass::Diagnostic,
        ),
        None,
    );

    assert!(outbound.is_empty());
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("diagnostic routes")),
        "diagnostic rejection should be visible; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

#[test]
fn publish_signed_event_rejects_private_kind_with_imported_presigned_route() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let raw = signed_kind_1059_raw(&id);

    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(
            vec!["wss://import.example".to_string()],
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );

    assert!(outbound.is_empty());
    assert!(
        kernel
            .last_error_toast_snapshot()
            .is_some_and(|t| t.contains("verified_private_inbox") && t.contains("D10")),
        "private pre-signed route must still require verified inbox; got: {:?}",
        kernel.last_error_toast_snapshot()
    );
}

/// The corresponding HAPPY path — a kind:1059 publish with a verified private
/// inbox pin must succeed unchanged.
#[test]
fn publish_signed_event_publishes_kind_1059_with_explicit_pin() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let raw = signed_kind_1059_raw(&id);

    let pin: Vec<String> = TEST_GROUP_RELAYS.iter().map(|s| s.to_string()).collect();
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::explicit(pin.clone(), PublishRouteClass::VerifiedPrivateInbox),
        None,
    );

    assert!(
        !outbound.is_empty(),
        "kind:1059 + verified private inbox pin must publish"
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
