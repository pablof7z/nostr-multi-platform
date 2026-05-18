//! Integration tests for the NIP-42 handshake driver.
//!
//! Lives outside `src/flow.rs` to keep the implementation file under the
//! AGENTS.md 300-LOC soft cap. Exercises the crate's public API only —
//! `Nip42Driver`, `RelayAuthState`, `AuthChallenge`, `AuthOk`,
//! `Nip42Error`, `HandshakeOutcome`, `build_auth_event`, `run_handshake`.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nmp_nip42::{
    build_auth_event, run_handshake, AuthChallenge, AuthOk, HandshakeOutcome, Nip42Driver,
    Nip42Error, RelayAuthState,
};

fn challenge_for(relay: &str, challenge: &str) -> AuthChallenge {
    AuthChallenge {
        challenge: challenge.to_string(),
        relay_url: relay.to_string(),
    }
}

fn good_signer_returning(
    id: &str,
) -> impl FnMut(&UnsignedEvent) -> Result<SignedEvent, Nip42Error> {
    let id = id.to_string();
    move |unsigned| {
        Ok(SignedEvent {
            id: id.clone(),
            sig: "c".repeat(128),
            unsigned: unsigned.clone(),
        })
    }
}

#[test]
fn happy_path_drives_through_full_lifecycle() {
    let mut driver = Nip42Driver::new();
    assert_eq!(*driver.state(), RelayAuthState::NotRequired);

    let ch = challenge_for("wss://r", "abc");
    let outcome = driver.on_auth_frame(ch.clone());
    assert_eq!(outcome.new_state, Some(RelayAuthState::ChallengeReceived));
    assert!(outcome.wire_frames.is_empty());
    assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);

    let id = "a".repeat(64);
    let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
    let signed = SignedEvent {
        id: id.clone(),
        sig: "c".repeat(128),
        unsigned,
    };
    let outcome = driver.deliver_signed(Ok(signed));
    assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
    assert_eq!(outcome.wire_frames.len(), 1);
    assert!(outcome.wire_frames[0].starts_with("[\"AUTH\","));
    assert_eq!(*driver.state(), RelayAuthState::Authenticating);

    let ok = AuthOk {
        event_id: id,
        accepted: true,
        reason: String::new(),
    };
    let outcome = driver.on_ok_frame(&ok);
    assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticated));
    assert!(outcome.wire_frames.is_empty());
    assert_eq!(*driver.state(), RelayAuthState::Authenticated);
    assert!(outcome.failure_reason.is_none());
}

#[test]
fn rejected_ok_surfaces_reason_and_transitions_to_failed() {
    let mut driver = Nip42Driver::new();
    let ch = challenge_for("wss://r", "x");
    driver.on_auth_frame(ch.clone());
    let id = "b".repeat(64);
    let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
    let signed = SignedEvent {
        id: id.clone(),
        sig: "c".repeat(128),
        unsigned,
    };
    driver.deliver_signed(Ok(signed));

    let ok = AuthOk {
        event_id: id,
        accepted: false,
        reason: "restricted: subscribers only".to_string(),
    };
    let outcome = driver.on_ok_frame(&ok);
    assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
    assert_eq!(*driver.state(), RelayAuthState::Failed);
    assert!(outcome.failure_reason.unwrap().contains("restricted"));
}

#[test]
fn signer_failure_surfaces_without_dispatching_wire_frame() {
    let mut driver = Nip42Driver::new();
    let ch = challenge_for("wss://r", "x");
    driver.on_auth_frame(ch);
    let outcome =
        driver.deliver_signed(Err(Nip42Error::SignerFailed("keychain locked".to_string())));
    assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
    assert!(outcome.wire_frames.is_empty());
    assert!(outcome.failure_reason.unwrap().contains("keychain locked"));
    assert_eq!(*driver.state(), RelayAuthState::Failed);
}

#[test]
fn signer_returning_invalid_event_is_treated_as_failure() {
    let mut driver = Nip42Driver::new();
    let ch = challenge_for("wss://r", "x");
    driver.on_auth_frame(ch);
    let bad_signed = SignedEvent {
        id: "a".repeat(64),
        sig: "c".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "b".repeat(64),
            kind: 1,
            tags: vec![],
            content: String::new(),
            created_at: 1,
        },
    };
    let outcome = driver.deliver_signed(Ok(bad_signed));
    assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
    assert!(outcome.wire_frames.is_empty());
    assert!(outcome.failure_reason.unwrap().contains("expected 22242"));
}

#[test]
fn unrelated_ok_does_not_change_state() {
    let mut driver = Nip42Driver::new();
    let ch = challenge_for("wss://r", "x");
    driver.on_auth_frame(ch.clone());
    let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
    driver.deliver_signed(Ok(SignedEvent {
        id: "1".repeat(64),
        sig: "c".repeat(128),
        unsigned,
    }));
    assert_eq!(*driver.state(), RelayAuthState::Authenticating);

    let other = AuthOk {
        event_id: "9".repeat(64),
        accepted: true,
        reason: String::new(),
    };
    let outcome = driver.on_ok_frame(&other);
    assert_eq!(outcome, HandshakeOutcome::default());
    assert_eq!(*driver.state(), RelayAuthState::Authenticating);
}

#[test]
fn reset_on_disconnect_clears_state_and_challenge() {
    let mut driver = Nip42Driver::new();
    driver.on_auth_frame(challenge_for("wss://r", "x"));
    driver.reset_on_disconnect();
    assert_eq!(*driver.state(), RelayAuthState::NotRequired);
    assert!(driver.pending_challenge().is_none());
}

#[test]
fn re_auth_after_authenticated_drops_back_to_challenge_received() {
    let mut driver = Nip42Driver::new();
    let ch1 = challenge_for("wss://r", "first");
    let id1 = "1".repeat(64);
    run_handshake(
        &mut driver,
        ch1.clone(),
        "p".repeat(64),
        1,
        good_signer_returning(&id1),
    );
    driver.on_ok_frame(&AuthOk {
        event_id: id1,
        accepted: true,
        reason: String::new(),
    });
    assert_eq!(*driver.state(), RelayAuthState::Authenticated);

    let ch2 = challenge_for("wss://r", "second");
    let outcome = driver.on_auth_frame(ch2);
    assert_eq!(outcome.new_state, Some(RelayAuthState::ChallengeReceived));
    assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
}

#[test]
fn run_handshake_dispatches_through_signer_in_one_call() {
    let mut driver = Nip42Driver::new();
    let id = "7".repeat(64);
    let outcome = run_handshake(
        &mut driver,
        challenge_for("wss://r", "ch"),
        "p".repeat(64),
        1,
        good_signer_returning(&id),
    );
    assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
    assert_eq!(outcome.wire_frames.len(), 1);
    assert!(outcome.wire_frames[0].contains(&id));
}

#[test]
fn deliver_signed_for_rejects_stale_signer_result() {
    let mut driver = Nip42Driver::new();
    let ch1 = challenge_for("wss://r", "first");
    driver.on_auth_frame(ch1.clone());
    let ch2 = challenge_for("wss://r", "second");
    driver.on_auth_frame(ch2);
    assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
    let id = "1".repeat(64);
    let mut signer = good_signer_returning(&id);
    let stale = signer(&build_auth_event(&ch1, "p".repeat(64), 1));
    let outcome = driver.deliver_signed_for(&ch1.challenge, stale);
    assert_eq!(
        outcome,
        HandshakeOutcome::default(),
        "stale signer result must be discarded"
    );
    assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
    assert_eq!(
        driver.pending_challenge().unwrap().challenge,
        "second",
        "current challenge unchanged by stale delivery"
    );
}

#[test]
fn deliver_signed_for_accepts_current_challenge() {
    let mut driver = Nip42Driver::new();
    let ch = challenge_for("wss://r", "live");
    driver.on_auth_frame(ch.clone());
    let id = "8".repeat(64);
    let mut signer = good_signer_returning(&id);
    let signed = signer(&build_auth_event(&ch, "p".repeat(64), 1));
    let outcome = driver.deliver_signed_for(&ch.challenge, signed);
    assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
    assert_eq!(outcome.wire_frames.len(), 1);
    assert_eq!(*driver.state(), RelayAuthState::Authenticating);
}

#[test]
fn deliver_signed_without_pending_challenge_is_noop() {
    let mut driver = Nip42Driver::new();
    let outcome = driver.deliver_signed(Ok(SignedEvent {
        id: "a".repeat(64),
        sig: "c".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "b".repeat(64),
            kind: 22242,
            tags: vec![],
            content: String::new(),
            created_at: 1,
        },
    }));
    assert_eq!(outcome, HandshakeOutcome::default());
    assert_eq!(*driver.state(), RelayAuthState::NotRequired);
}
