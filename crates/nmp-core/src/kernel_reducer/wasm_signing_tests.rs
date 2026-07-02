//! #1753 S6 — functional tests for the wasm signing capability round-trip.
//!
//! These run on native (`cargo test -p nmp-core`) AND wasm (`cargo test -p
//! nmp-browser-runtime` exercises the public `KernelReducer` seam) — the seam is
//! target-agnostic. They pin the round-trip shape, account-pinning, the
//! sign-only reducer terminal, and the totality (D6) contracts. The
//! NO-POLLING proof lives in the sibling `no_polling_oracle_tests` module.

use super::SignRoundTripOutcome;
use crate::kernel_reducer::KernelReducer;
use nmp_signer_iface::UnsignedEvent;

const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const OTHER_ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn unsigned_json(pubkey: &str) -> String {
    serde_json::to_string(&UnsignedEvent {
        pubkey: pubkey.to_string(),
        kind: 1,
        tags: vec![],
        content: "hello from wasm sign roundtrip".to_string(),
        created_at: 1_700_000_000,
    })
    .unwrap()
}

/// Build a flat-NIP-01 signed event JSON authored by `pubkey` (what the broker
/// would post back after `window.nostr.signEvent`).
fn signed_flat_json(pubkey: &str, content: &str) -> String {
    serde_json::json!({
        "id": "11".repeat(32),
        "pubkey": pubkey,
        "created_at": 1_700_000_000u64,
        "kind": 1,
        "tags": [],
        "content": content,
        "sig": "22".repeat(64),
    })
    .to_string()
}

/// The happy path: begin parks a request, delivering the matching signed event
/// resolves it via message re-entry, and the completion records the signed JSON
/// for the owning runtime or host to consume.
#[test]
fn begin_then_deliver_completes_roundtrip() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .expect("begin must succeed for valid unsigned JSON");
    assert_eq!(req.account_pubkey, ACCOUNT);
    assert_eq!(r.pending_sign_roundtrips(), 1, "one op parked after begin");

    let signed = signed_flat_json(ACCOUNT, "hello from wasm sign roundtrip");
    let outcome = r.deliver_signed_response(&req.correlation_id, &signed);
    match outcome {
        SignRoundTripOutcome::Completed {
            correlation_id,
            signed_json,
        } => {
            assert_eq!(correlation_id, req.correlation_id);
            assert!(
                signed_json.contains("\"sig\":\"2222"),
                "completion carries the signed flat JSON"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert_eq!(
        r.pending_sign_roundtrips(),
        0,
        "op dropped after completion"
    );

    let completions = r.take_sign_completions();
    assert_eq!(completions.len(), 1, "exactly one completion recorded");
    assert!(completions[0].outcome.is_ok(), "the round-trip recorded Ok");
    assert!(
        r.take_sign_completions().is_empty(),
        "completions are drained on read"
    );
}

/// Account-pinning: a signature authored by a DIFFERENT account than the one the
/// request was pinned to is rejected (no cross-delivery). ADR-0072 §D5.
#[test]
fn account_pin_mismatch_is_rejected() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    // Broker posts back a signature authored by a different key.
    let signed = signed_flat_json(&OTHER_ACCOUNT, "hello from wasm sign roundtrip");
    let outcome = r.deliver_signed_response(&req.correlation_id, &signed);
    match outcome {
        SignRoundTripOutcome::Failed { reason, .. } => {
            assert!(
                reason.contains("account-pin mismatch"),
                "mismatch must be reported as a pin failure; got {reason:?}"
            );
        }
        other => panic!("expected Failed (pin mismatch), got {other:?}"),
    }
    assert_eq!(
        r.pending_sign_roundtrips(),
        0,
        "the op is still resolved (D6)"
    );
}

/// A malformed signed-event JSON resolves the op with an error terminal — the
/// op never dangles (D6).
#[test]
fn malformed_signed_json_fails_closed() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    let outcome = r.deliver_signed_response(&req.correlation_id, "not json at all");
    assert!(
        matches!(outcome, SignRoundTripOutcome::Failed { .. }),
        "garbage JSON fails the round-trip closed, got {outcome:?}"
    );
    assert_eq!(r.pending_sign_roundtrips(), 0);
}

/// `fail_sign_roundtrip` (user rejected in the extension / no `window.nostr`)
/// resolves the parked op with the supplied reason.
#[test]
fn explicit_failure_resolves_op() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    let outcome = r.fail_sign_roundtrip(&req.correlation_id, "user rejected in extension");
    match outcome {
        SignRoundTripOutcome::Failed { reason, .. } => {
            assert!(
                reason.contains("user rejected"),
                "reason propagated; got {reason:?}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(r.pending_sign_roundtrips(), 0);
}

/// Delivering for an unknown correlation id (stale / duplicate) is a no-op.
#[test]
fn unknown_correlation_is_a_noop() {
    let mut r = KernelReducer::new();
    let outcome = r.deliver_signed_response("never-parked", &signed_flat_json(ACCOUNT, "x"));
    assert!(
        matches!(outcome, SignRoundTripOutcome::Unknown { .. }),
        "no parked op → Unknown, got {outcome:?}"
    );
    // A duplicate delivery after a real completion is also Unknown (the sender
    // was consumed on the first delivery).
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    let _ = r.deliver_signed_response(
        &req.correlation_id,
        &signed_flat_json(ACCOUNT, "hello from wasm sign roundtrip"),
    );
    let dup = r.deliver_signed_response(
        &req.correlation_id,
        &signed_flat_json(ACCOUNT, "hello from wasm sign roundtrip"),
    );
    assert!(
        matches!(dup, SignRoundTripOutcome::Unknown { .. }),
        "duplicate delivery is Unknown, got {dup:?}"
    );
}

/// Two concurrent in-flight round-trips resolve independently and to the right
/// correlation id (no cross-talk through the shared queue).
#[test]
fn concurrent_roundtrips_resolve_independently() {
    let mut r = KernelReducer::new();
    let req_a = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    let req_b = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json(ACCOUNT))
        .unwrap();
    assert_ne!(req_a.correlation_id, req_b.correlation_id);
    assert_eq!(r.pending_sign_roundtrips(), 2);

    // Deliver B first, then A — order-independent.
    let ob = r.deliver_signed_response(&req_b.correlation_id, &signed_flat_json(ACCOUNT, "b"));
    assert!(matches!(ob, SignRoundTripOutcome::Completed { .. }));
    assert_eq!(r.pending_sign_roundtrips(), 1);
    let oa = r.deliver_signed_response(&req_a.correlation_id, &signed_flat_json(ACCOUNT, "a"));
    assert!(matches!(oa, SignRoundTripOutcome::Completed { .. }));
    assert_eq!(r.pending_sign_roundtrips(), 0);
}
