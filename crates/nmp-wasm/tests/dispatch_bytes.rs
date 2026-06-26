//! ADR-0064 / S2 (#1750) + S3 (#1751) — the wasm binary write doorway
//! (`dispatch_bytes`).
//!
//! S2 proves the raw-byte inbound channel: a transferable `Uint8Array` of a
//! finished `DispatchEnvelope` crosses, decodes through the ONE core decode
//! path, and routes by `action_namespace`. Fail-closed cases surface a
//! data-shaped `WorkerEvent::Error` — never a panic, never a silent accept.
//!
//! This file covers the S2 envelope decode/route cases plus the publish
//! sign-roundtrip path. The S3 (#1751 / #1008) typed-payload DECODE arm — a
//! registered non-publish namespace's typed payload reaching the module's
//! `decode_payload` + `start()` validator — lives in the sibling
//! `dispatch_bytes_commands.rs` (file-size ceiling split).

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_core::substrate::ActionPayload;
use nmp_wasm::{CapabilityFailure, RawWasmAbiAdapter, SetIdentity, WorkerEvent, WorkerRequest};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

fn envelope(namespace: &str, correlation: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    encode_dispatch_envelope(correlation, namespace, version, payload)
}

/// Seed an active account so a dispatch passes the fail-closed
/// `signer_not_installed` gate and reaches the typed registry.
fn seed_account(runtime: &mut RawWasmAbiAdapter) {
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: PK.to_string(),
            correlation_id: "seed".to_string(),
            identity_relays: Vec::new(),
        }))
        .expect("set_identity must succeed");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::ActionAccepted { .. })),
        "seed_account: SetIdentity must ACK; got {events:?}"
    );
}

fn publish_raw_payload(content: &str) -> Vec<u8> {
    PublishAction::PublishRaw {
        kind: 1,
        tags: Vec::new(),
        content: content.to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    }
    .encode()
}

fn sign_request_correlation(events: &[WorkerEvent]) -> String {
    events
        .iter()
        .find_map(|event| match event {
            WorkerEvent::SignRequest { correlation_id, .. } => Some(correlation_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected SignRequest, got {events:?}"))
}

/// Seed an active account using an explicit pubkey (for tests that generate
/// fresh test keys instead of using the shared `PK` constant).
fn seed_account_with_pubkey(runtime: &mut RawWasmAbiAdapter, pubkey_hex: &str) {
    let events = runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: pubkey_hex.to_string(),
            correlation_id: "seed".to_string(),
            identity_relays: Vec::new(),
        }))
        .expect("set_identity must succeed");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, WorkerEvent::ActionAccepted { .. })),
        "seed_account_with_pubkey: SetIdentity must ACK; got {events:?}"
    );
}

/// Build a properly Schnorr-signed event JSON for the `deliver_signer_response`
/// flow. `publish_pre_signed` now verifies signatures (#2045 PR-A), so tests
/// exercising that path need a real signed event rather than a placeholder.
///
/// The pubkey in the returned JSON is `keys.public_key()` — callers MUST seed
/// the active account with the same pubkey so the account-pin check in
/// `deliver_signed_response_at` passes.
fn real_signed_event_json(keys: &nostr::Keys, content: &str) -> String {
    let event = nostr::EventBuilder::new(nostr::Kind::from(1u16), content)
        .custom_created_at(nostr::Timestamp::from_secs(1_700_001_234))
        .sign_with_keys(keys)
        .expect("test keys sign");
    serde_json::json!({
        "id": event.id.to_hex(),
        "pubkey": event.pubkey.to_hex(),
        "created_at": event.created_at.as_secs(),
        "kind": 1u16,
        "tags": serde_json::Value::Array(vec![]),
        "content": event.content,
        "sig": event.sig.to_string(),
    })
    .to_string()
}

#[test]
fn binary_doorway_decodes_and_routes_by_namespace() {
    let mut runtime = RawWasmAbiAdapter::new();
    // Opaque payload — the binary lane carries it verbatim, never interpreted.
    let bytes = envelope(
        "nmp.publish",
        "corr-1",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        b"\x01\x02\x03",
    );

    let events = runtime.dispatch_bytes(&bytes);

    // No signer installed → the honest write-path reason, keyed to the decoded
    // namespace + correlation_id. Proves the envelope crossed and decoded.
    assert!(matches!(
        &events[..],
        [WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability,
            correlation_id,
            ..
        })] if capability == "nmp.publish" && correlation_id == "corr-1"
    ));
}

#[test]
fn binary_doorway_rejects_schema_version_mismatch_fail_closed() {
    let mut runtime = RawWasmAbiAdapter::new();
    let bad = envelope(
        "nmp.publish",
        "corr-2",
        DISPATCH_ENVELOPE_SCHEMA_VERSION + 1,
        b"p",
    );

    let events = runtime.dispatch_bytes(&bad);

    // Fail closed: a data-shaped error, NOT a routed dispatch.
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_wrong_file_identifier_fail_closed() {
    let mut runtime = RawWasmAbiAdapter::new();
    let mut bytes = envelope("nmp.publish", "c", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    bytes[4..8].copy_from_slice(b"NMPU"); // read-direction magic
    let events = runtime.dispatch_bytes(&bytes);
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn binary_doorway_rejects_garbage_fail_closed() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime.dispatch_bytes(b"not a flatbuffer");
    assert!(matches!(
        &events[..],
        [WorkerEvent::Error { code, .. }] if code == "dispatch_envelope_rejected"
    ));
}

#[test]
fn publish_raw_unsigned_json_uses_reducer_clock_for_created_at() {
    let mut runtime = RawWasmAbiAdapter::new();
    seed_account(&mut runtime);
    runtime.set_kernel_clock_for_test(Arc::new(nmp_core::MonotonicSecondClock::new(
        UNIX_EPOCH + Duration::from_secs(1_700_001_234),
    )));

    let payload = publish_raw_payload("clock-owned publish");
    let bytes = envelope(
        "nmp.publish",
        "corr-clock",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let events = runtime.dispatch_bytes(&bytes);
    let unsigned_json = events
        .iter()
        .find_map(|event| {
            if let WorkerEvent::SignRequest { unsigned_json, .. } = event {
                Some(unsigned_json)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("expected SignRequest from PublishRaw; got {events:?}"));
    let value: serde_json::Value =
        serde_json::from_str(unsigned_json).expect("unsigned JSON must decode");
    assert_eq!(
        value["created_at"].as_u64(),
        Some(1_700_001_234),
        "PublishRaw created_at must come from the reducer-owned kernel clock"
    );
}

#[test]
fn publish_raw_deliver_signer_response_publishes_signed_event() {
    // Use generated test keys so the account-pin check and Schnorr signature
    // verification both pass (#2045 PR-A: `publish_pre_signed` now verifies
    // signatures, closing the forged-event gap on the wasm path).
    let test_keys = nostr::Keys::generate();
    let mut runtime = RawWasmAbiAdapter::new();
    seed_account_with_pubkey(&mut runtime, &test_keys.public_key().to_hex());
    let start_events = runtime
        .handle(WorkerRequest::Start(nmp_wasm::StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            relay_bootstrap: vec![nmp_wasm::RelayBootstrapEntry {
                url: "wss://relay.example".to_string(),
                role: "both".to_string(),
            }],
            database_name: "publish-continuation-test".to_string(),
            correlation_id: "start".to_string(),
        }))
        .expect("start must succeed");
    assert!(
        start_events
            .iter()
            .any(|event| matches!(event, WorkerEvent::RuntimeStatus { .. })),
        "Start must return runtime status; got {start_events:?}"
    );
    let _ = runtime
        .fire_maintenance_deadline_for_test()
        .expect("clear post-start event drain");

    let payload = publish_raw_payload("publish continuation");
    let bytes = envelope(
        "nmp.publish",
        "corr-publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let events = runtime.dispatch_bytes(&bytes);
    let sign_correlation = sign_request_correlation(&events);

    let completion_events = runtime
        .handle(WorkerRequest::DeliverSignerResponse(
            nmp_wasm::DeliverSignerResponse {
                correlation_id: sign_correlation.clone(),
                signed_json: Some(real_signed_event_json(&test_keys, "publish continuation")),
                error: None,
            },
        ))
        .expect("deliver_signer_response must succeed");

    assert!(
        completion_events.iter().any(|event| matches!(
            event,
            WorkerEvent::SignCompleted { correlation_id, .. } if correlation_id == &sign_correlation
        )),
        "sign completion must still be observable; got {completion_events:?}"
    );
    assert!(
        completion_events.iter().any(|event| matches!(
            event,
            WorkerEvent::ActionAccepted { action_type, correlation_id }
                if action_type == "nmp.publish" && correlation_id == "corr-publish"
        )),
        "signed publish must ACK the original action correlation; got {completion_events:?}"
    );
    assert!(
        runtime.next_runtime_deadline_delay_for_test().is_some(),
        "deliver_signer_response must route the signed event through publish_pre_signed \
         and arm the publish deadline"
    );
}
