//! #1753 S6 — wasm signing capability round-trip (pure message re-entry).
//!
//! These run on native (`cargo test -p nmp-wasm`) through the same
//! `RawWasmAbiAdapter::handle` path the wasm32 worker uses. They exercise the full
//! protocol round-trip — `BeginSign` → `SignRequest` (broker request) →
//! `DeliverSignerResponse` (broker fulfilment) → `SignCompleted` — and pin the
//! no-polling property at the runtime boundary: only the delivery message
//! completes the round-trip.
//!
//! The LIVE `window.nostr` browser call is CI/manual-gated (no browser here);
//! these tests cover the Rust/wasm core + the JS-bridge message contract.

use nmp_wasm::{BeginSign, DeliverSignerResponse, RawWasmAbiAdapter, WorkerEvent, WorkerRequest};
use serde_json::json;

const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const OTHER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn unsigned_json(pubkey: &str) -> String {
    json!({
        "pubkey": pubkey,
        "kind": 1,
        "tags": [],
        "content": "wasm roundtrip note",
        "created_at": 1_700_000_000u64,
    })
    .to_string()
}

fn signed_json(pubkey: &str) -> String {
    json!({
        "id": "11".repeat(32),
        "pubkey": pubkey,
        "created_at": 1_700_000_000u64,
        "kind": 1,
        "tags": [],
        "content": "wasm roundtrip note",
        "sig": "22".repeat(64),
    })
    .to_string()
}

/// The protocol messages JSON-round-trip (the JS bridge speaks JSON).
#[test]
fn sign_messages_round_trip_through_json() {
    let begin = WorkerRequest::BeginSign(BeginSign {
        account_pubkey: ACCOUNT.to_string(),
        unsigned_json: unsigned_json(ACCOUNT),
    });
    let decoded: WorkerRequest =
        serde_json::from_str(&serde_json::to_string(&begin).unwrap()).unwrap();
    assert_eq!(decoded, begin);

    let deliver = WorkerRequest::DeliverSignerResponse(DeliverSignerResponse {
        correlation_id: "c1".to_string(),
        signed_json: Some(signed_json(ACCOUNT)),
        error: None,
    });
    let decoded: WorkerRequest =
        serde_json::from_str(&serde_json::to_string(&deliver).unwrap()).unwrap();
    assert_eq!(decoded, deliver);
}

/// Full round-trip: BeginSign emits a SignRequest for the broker; delivering the
/// signed bytes back emits SignCompleted carrying the signed JSON.
#[test]
fn begin_sign_then_deliver_completes() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::BeginSign(BeginSign {
            account_pubkey: ACCOUNT.to_string(),
            unsigned_json: unsigned_json(ACCOUNT),
        }))
        .unwrap();
    let correlation_id = match events.as_slice() {
        [WorkerEvent::SignRequest {
            correlation_id,
            account_pubkey,
            unsigned_json,
        }] => {
            assert_eq!(account_pubkey, ACCOUNT, "request is pinned to the account");
            assert!(unsigned_json.contains("wasm roundtrip note"));
            correlation_id.clone()
        }
        other => panic!("expected a single SignRequest, got {other:?}"),
    };

    // The main-thread broker calls window.nostr.signEvent and posts the result
    // back. (Here we synthesize what the bridge would return.)
    let events = runtime
        .handle(WorkerRequest::DeliverSignerResponse(
            DeliverSignerResponse {
                correlation_id: correlation_id.clone(),
                signed_json: Some(signed_json(ACCOUNT)),
                error: None,
            },
        ))
        .unwrap();
    match events.as_slice() {
        [WorkerEvent::SignCompleted {
            correlation_id: cid,
            signed_json,
        }] => {
            assert_eq!(*cid, correlation_id);
            assert!(
                signed_json.contains("\"sig\":\"2222"),
                "carries the signed event"
            );
        }
        other => panic!("expected SignCompleted, got {other:?}"),
    }
}

/// Account-pinning across the bridge: a signature authored by a different
/// account than the round-trip was begun for is rejected (ADR-0050 §D5).
#[test]
fn account_pin_enforced_across_the_bridge() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::BeginSign(BeginSign {
            account_pubkey: ACCOUNT.to_string(),
            unsigned_json: unsigned_json(ACCOUNT),
        }))
        .unwrap();
    let correlation_id = match &events[0] {
        WorkerEvent::SignRequest { correlation_id, .. } => correlation_id.clone(),
        other => panic!("expected SignRequest, got {other:?}"),
    };
    let events = runtime
        .handle(WorkerRequest::DeliverSignerResponse(
            DeliverSignerResponse {
                correlation_id,
                // Broker returns a signature from the WRONG account.
                signed_json: Some(signed_json(OTHER)),
                error: None,
            },
        ))
        .unwrap();
    match &events[0] {
        WorkerEvent::SignFailed { reason, .. } => {
            assert!(reason.contains("account-pin mismatch"), "got {reason:?}");
        }
        other => panic!("expected SignFailed (pin mismatch), got {other:?}"),
    }
}

/// The broker reports a user rejection (`error` set) — the round-trip fails
/// closed with that reason (D6).
#[test]
fn broker_reported_rejection_fails_closed() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::BeginSign(BeginSign {
            account_pubkey: ACCOUNT.to_string(),
            unsigned_json: unsigned_json(ACCOUNT),
        }))
        .unwrap();
    let correlation_id = match &events[0] {
        WorkerEvent::SignRequest { correlation_id, .. } => correlation_id.clone(),
        other => panic!("expected SignRequest, got {other:?}"),
    };
    let events = runtime
        .handle(WorkerRequest::DeliverSignerResponse(
            DeliverSignerResponse {
                correlation_id,
                signed_json: None,
                error: Some("user rejected in extension".to_string()),
            },
        ))
        .unwrap();
    match &events[0] {
        WorkerEvent::SignFailed { reason, .. } => {
            assert!(reason.contains("user rejected"), "got {reason:?}");
        }
        other => panic!("expected SignFailed, got {other:?}"),
    }
}

/// NO-POLLING at the runtime boundary: the periodic `tick` request equivalent
/// does not exist as a `WorkerRequest`, and crucially, nothing but the delivery
/// message completes the round-trip. We begin a sign, then drive every OTHER
/// request the runtime accepts (a no-op CapabilityResult) and assert the
/// round-trip stays open — only `DeliverSignerResponse` closes it.
#[test]
fn only_the_delivery_message_completes_the_roundtrip() {
    let mut runtime = RawWasmAbiAdapter::new();
    let events = runtime
        .handle(WorkerRequest::BeginSign(BeginSign {
            account_pubkey: ACCOUNT.to_string(),
            unsigned_json: unsigned_json(ACCOUNT),
        }))
        .unwrap();
    let correlation_id = match &events[0] {
        WorkerEvent::SignRequest { correlation_id, .. } => correlation_id.clone(),
        other => panic!("expected SignRequest, got {other:?}"),
    };

    // Drive an unrelated capability-result message many times. None of these may
    // produce a SignCompleted / SignFailed for our correlation id — completion
    // is gated solely on the delivery message (D8, no polling).
    for _ in 0..50 {
        let events = runtime
            .handle(WorkerRequest::CapabilityResult(
                nmp_wasm::CapabilityResult {
                    capability: "unrelated".to_string(),
                    correlation_id: "unrelated".to_string(),
                    payload: json!({}),
                },
            ))
            .unwrap();
        assert!(
            !events.iter().any(|e| matches!(
                e,
                WorkerEvent::SignCompleted { correlation_id: c, .. }
                    | WorkerEvent::SignFailed { correlation_id: c, .. }
                    if *c == correlation_id
            )),
            "no non-delivery message may complete the parked sign (no polling)"
        );
    }

    // Now the delivery message — and ONLY now — completes it.
    let events = runtime
        .handle(WorkerRequest::DeliverSignerResponse(
            DeliverSignerResponse {
                correlation_id: correlation_id.clone(),
                signed_json: Some(signed_json(ACCOUNT)),
                error: None,
            },
        ))
        .unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            WorkerEvent::SignCompleted { correlation_id: c, .. } if *c == correlation_id
        )),
        "the delivery message completes the round-trip"
    );
}
