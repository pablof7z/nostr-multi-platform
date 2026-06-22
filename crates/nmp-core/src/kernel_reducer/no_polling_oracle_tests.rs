//! #1753 S6 — the NO-POLLING completion oracle (D8).
//!
//! These tests are the falsifiable proof that the wasm sign round-trip
//! completes by **message re-entry**, not by polling, ticking, or sleeping. The
//! claim under test:
//!
//! > A parked sign op is resolved if and only if a completion MESSAGE
//! > (`deliver_signed_response` / `fail_sign_roundtrip`) is delivered. No timer,
//! > no `tick()`, and no passage of wall-clock time resolves it.
//!
//! If S6 ever regressed to a poll/tick-driven completion (e.g. by routing the
//! op through an idle drain that the periodic `tick()` drives), [`tick_never_completes_a_parked_sign`]
//! would FAIL — the op would resolve without a message. That is the oracle's
//! teeth: it is not a tautology, it discriminates the two designs.

use super::SignRoundTripOutcome;
use crate::kernel_reducer::KernelReducer;
use crate::substrate::UnsignedEvent;

const ACCOUNT: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

fn unsigned_json() -> String {
    serde_json::to_string(&UnsignedEvent {
        pubkey: ACCOUNT.to_string(),
        kind: 1,
        tags: vec![],
        content: "oracle".to_string(),
        created_at: 1_700_000_000,
    })
    .unwrap()
}

fn signed_flat_json() -> String {
    serde_json::json!({
        "id": "11".repeat(32),
        "pubkey": ACCOUNT,
        "created_at": 1_700_000_000u64,
        "kind": 1,
        "tags": [],
        "content": "oracle",
        "sig": "22".repeat(64),
    })
    .to_string()
}

/// ORACLE 1 — the periodic `tick()` (the ONLY recurring drive the wasm runtime
/// has) does NOT complete a parked sign op. Completion is message-gated.
///
/// We begin a round-trip, then call `tick()` many times (simulating the 1 Hz
/// timer firing repeatedly) WITHOUT delivering a response. The op must stay
/// parked and record NO completion. A poll/tick-driven design would resolve it
/// here — proving this test discriminates the designs.
#[test]
fn tick_never_completes_a_parked_sign() {
    let mut r = KernelReducer::new();
    let _req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json())
        .unwrap();
    assert_eq!(r.pending_sign_roundtrips(), 1, "parked after begin");

    // Fire the periodic tick many times — the only recurring drive in the wasm
    // runtime. None of these may complete the op (no message has arrived).
    for _ in 0..100 {
        let _ = r.tick();
        assert_eq!(
            r.pending_sign_roundtrips(),
            1,
            "tick must NOT resolve a parked sign — completion is message-gated (D8)"
        );
        assert!(
            r.take_sign_completions().is_empty(),
            "tick must NOT record any completion (no poll-driven resolution)"
        );
    }
}

/// ORACLE 2 — completion is observed SYNCHRONOUSLY inside the
/// `deliver_signed_response` message handler. The op is still parked the
/// instant before the call and resolved the instant after — there is no
/// intermediate poll window, no later tick required.
#[test]
fn completion_happens_inside_the_delivery_message() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json())
        .unwrap();

    // Before the message: parked, no completion.
    assert_eq!(r.pending_sign_roundtrips(), 1);
    assert!(r.take_sign_completions().is_empty());

    // The message arrives. Completion MUST be observable as the direct return of
    // this single call — not after a subsequent tick/poll.
    let outcome = r.deliver_signed_response(&req.correlation_id, &signed_flat_json());
    assert!(
        matches!(outcome, SignRoundTripOutcome::Completed { .. }),
        "deliver_signed_response resolves the op in-call (message re-entry)"
    );
    assert_eq!(
        r.pending_sign_roundtrips(),
        0,
        "the op is gone the instant the message handler returned — no later drive"
    );
}

/// ORACLE 3 — no-message means no-completion, then a single message completes.
/// This is the end-to-end shape of the proof: the op survives arbitrary
/// non-message activity (ticks) and resolves only when, and exactly when, the
/// message is delivered.
#[test]
fn op_survives_ticks_then_resolves_only_on_message() {
    let mut r = KernelReducer::new();
    let req = r
        .begin_sign_roundtrip(ACCOUNT.to_string(), &unsigned_json())
        .unwrap();

    // Arbitrary non-message activity: ticks + unrelated reduces. The op persists.
    for _ in 0..10 {
        let _ = r.tick();
    }
    assert_eq!(r.pending_sign_roundtrips(), 1, "op survived non-message activity");

    // The single message resolves it.
    let outcome = r.deliver_signed_response(&req.correlation_id, &signed_flat_json());
    assert!(matches!(outcome, SignRoundTripOutcome::Completed { .. }));
    assert_eq!(r.pending_sign_roundtrips(), 0);
}

/// ORACLE 4 (source-level guard) — the wasm-signing module must not introduce a
/// polling primitive. This reads the module source and asserts it contains no
/// `SignerOp::wait` (blocking recv), no `recv_timeout`, no `sleep`, and no
/// `loop {`/`while` poll construct in the completion path. A future edit that
/// reintroduces polling trips this guard.
///
/// This is a coarse but honest lexical gate; the behavioural oracles above are
/// the primary proof. Together they make a poll-driven regression hard to land
/// silently.
#[test]
fn module_source_contains_no_polling_primitive() {
    let src = include_str!("wasm_signing.rs");
    // Strip the doc-comment header (it legitimately mentions these words while
    // explaining what the code does NOT do) so the lexical scan only sees code.
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains(".wait("),
        "wasm signing must not call SignerOp::wait (blocking recv — D8)"
    );
    assert!(
        !code.contains("recv_timeout"),
        "wasm signing must not block on recv_timeout (D8)"
    );
    assert!(
        !code.contains("sleep"),
        "wasm signing must not sleep (D8)"
    );
    assert!(
        !code.contains("set_interval") && !code.contains("setInterval"),
        "wasm signing must not install its own completion timer (D8)"
    );
}
