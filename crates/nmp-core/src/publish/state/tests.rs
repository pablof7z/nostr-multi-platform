//! Inline unit tests for the pure pieces of the per-relay state machine.
//!
//! Scope note: the integration-style coverage of `apply_ack` over an
//! `InFlight` state (success / permanent / transient backoff ladder /
//! reauth ladder) already lives in `publish/tests.rs`, and the
//! code→`AckClass` mapping is exhaustively pinned there too. These tests
//! deliberately cover the gaps that file leaves: `is_terminal`, `attempt`,
//! `RetryPolicy` defaults + `backoff_for`, the non-`InFlight` stale-ack
//! guard in `apply_ack`, plus a minimal `classify_ack` smoke set so the
//! classifier is documented by test co-located with its definition.

use super::*;

// --- classify_ack: minimal smoke set (doc-by-test, not exhaustive) -----

#[test]
fn classify_ack_ok_is_permanent() {
    // ok=true never reaches the classifier via apply_ack, but the
    // classifier pins it to Permanent so an accidental call cannot
    // trigger a retry loop.
    assert_eq!(
        classify_ack(&RelayAck::ok("wss://relay.example/")),
        AckClass::Permanent
    );
}

#[test]
fn classify_ack_permanent_codes_are_permanent() {
    // "duplicate" and "blocked"/"spam"-class rejections classify as
    // permanent — the engine gives up immediately, no retry.
    for code in ["duplicate", "blocked"] {
        let ack = RelayAck::failed("wss://relay.example/", code, "rejected");
        assert_eq!(classify_ack(&ack), AckClass::Permanent, "code={code}");
    }
}

#[test]
fn classify_ack_auth_required_is_auth_required() {
    let ack = RelayAck::failed("wss://relay.example/", "auth-required", "please AUTH");
    assert_eq!(classify_ack(&ack), AckClass::AuthRequired);
}

#[test]
fn classify_ack_unknown_code_is_transient() {
    // Unknown / unrecognised tokens fall through to the conservative
    // retry verdict. A timeout is the canonical transport-class case.
    let unknown = RelayAck::failed("wss://relay.example/", "totally-unknown-token", "huh");
    assert_eq!(classify_ack(&unknown), AckClass::Transient);
    assert_eq!(
        classify_ack(&RelayAck::timed_out("wss://relay.example/")),
        AckClass::Transient
    );
}

// --- PerRelayState::is_terminal -----------------------------------------

#[test]
fn is_terminal_true_only_for_ok_and_failed_after_retries() {
    assert!(PerRelayState::Ok { acked_at_ms: 10 }.is_terminal());
    assert!(PerRelayState::FailedAfterRetries {
        reason: "gave up".into(),
        last_at_ms: 10,
    }
    .is_terminal());
}

#[test]
fn is_terminal_false_for_non_settled_states() {
    let non_terminal = [
        PerRelayState::Pending,
        PerRelayState::InFlight {
            sent_at_ms: 1,
            attempt: 1,
        },
        PerRelayState::RelayError {
            message: "transient".into(),
            attempt: 1,
            last_at_ms: 1,
        },
        PerRelayState::TimedOut {
            attempt: 1,
            last_at_ms: 1,
        },
    ];
    for state in non_terminal {
        assert!(!state.is_terminal(), "expected non-terminal: {state:?}");
    }
}

// --- PerRelayState::attempt ---------------------------------------------

#[test]
fn attempt_reports_zero_for_pending_and_settled_states() {
    assert_eq!(PerRelayState::Pending.attempt(), 0);
    assert_eq!(PerRelayState::Ok { acked_at_ms: 5 }.attempt(), 0);
    assert_eq!(
        PerRelayState::FailedAfterRetries {
            reason: "x".into(),
            last_at_ms: 5
        }
        .attempt(),
        0
    );
}

#[test]
fn attempt_reports_inner_counter_for_in_progress_states() {
    assert_eq!(
        PerRelayState::InFlight {
            sent_at_ms: 1,
            attempt: 2
        }
        .attempt(),
        2
    );
    assert_eq!(
        PerRelayState::RelayError {
            message: "e".into(),
            attempt: 3,
            last_at_ms: 1
        }
        .attempt(),
        3
    );
    assert_eq!(
        PerRelayState::TimedOut {
            attempt: 4,
            last_at_ms: 1
        }
        .attempt(),
        4
    );
}

// --- RetryPolicy ---------------------------------------------------------

#[test]
fn retry_policy_default_matches_documented_values() {
    let p = RetryPolicy::default();
    assert_eq!(p.transient_max_retries, 3);
    assert_eq!(p.backoff_base_ms, 1_000);
    assert_eq!(p.backoff_factor, 4);
}

#[test]
fn backoff_for_follows_documented_exponential_ladder() {
    // Docstring: 1s after attempt 1, 4s after attempt 2, 16s after 3.
    let p = RetryPolicy::default();
    assert_eq!(p.backoff_for(1), 1_000);
    assert_eq!(p.backoff_for(2), 4_000);
    assert_eq!(p.backoff_for(3), 16_000);
}

#[test]
fn backoff_for_saturates_instead_of_overflowing() {
    // A pathological attempt count must not panic on multiply overflow.
    let p = RetryPolicy::default();
    assert_eq!(p.backoff_for(1_000), u64::MAX);
}

// --- apply_ack: non-InFlight stale-ack guard ----------------------------

#[test]
fn apply_ack_on_terminal_ok_holds_settled_state_idempotently() {
    // A late ack for an already-settled relay must not mutate it.
    let settled = PerRelayState::Ok { acked_at_ms: 1_000 };
    let verdict = apply_ack(
        &settled,
        &RelayAck::ok("wss://relay.example/"),
        RetryPolicy::default(),
        9_999,
    );
    assert_eq!(verdict, RetryVerdict::Settled(settled));
}

#[test]
fn apply_ack_on_terminal_failure_holds_settled_state_idempotently() {
    let settled = PerRelayState::FailedAfterRetries {
        reason: "blocked: spam".into(),
        last_at_ms: 1_000,
    };
    let verdict = apply_ack(
        &settled,
        &RelayAck::failed("wss://relay.example/", "blocked", "blocked: spam"),
        RetryPolicy::default(),
        9_999,
    );
    assert_eq!(verdict, RetryVerdict::Settled(settled));
}

#[test]
fn apply_ack_on_pending_treats_ack_as_stale_duplicate() {
    // An ack arriving before the send was recorded as InFlight is stale;
    // the state is held unchanged rather than advanced.
    let verdict = apply_ack(
        &PerRelayState::Pending,
        &RelayAck::ok("wss://relay.example/"),
        RetryPolicy::default(),
        5_000,
    );
    assert_eq!(verdict, RetryVerdict::Settled(PerRelayState::Pending));
}
