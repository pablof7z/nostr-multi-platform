//! #3010 — `nutzap_await`'s two halves in isolation:
//!
//! - [`NutzapInfoArrivalParser`]: a kind:10019 from the awaited recipient
//!   redrives EVERY parked `SendNutzap` for that recipient exactly once
//!   (fresh journal operation, same caller correlation id); an unrelated
//!   pubkey/kind is a no-op; a second arrival after the first redrives
//!   nothing (already taken — at-most-once).
//! - [`run_ttl_sweep`] (the `Kernel`-free core of `NutzapAwaitTtlSweep::
//!   on_idle_tick`): fails closed exactly the awaits older than the bound,
//!   leaving fresher ones untouched.

use super::*;
use nmp_store::RawEvent;

fn verified_kind_10019(pubkey: &str) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_100,
        kind: nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    })
}

const RECIPIENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_PUBKEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FRESH_RECIPIENT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[test]
fn arrival_redrives_every_parked_send_for_that_recipient_exactly_once() {
    let state = Arc::new(Mutex::new(CashuWalletState::new()));
    {
        let mut s = lock_state(&state);
        s.park_send_await(
            RECIPIENT,
            "acct".to_string(),
            21,
            None,
            Some("cid-1".to_string()),
            100,
        );
        s.park_send_await(
            RECIPIENT,
            "acct".to_string(),
            7,
            None,
            Some("cid-2".to_string()),
            100,
        );
    }
    let (tx_raw, rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let tx = CommandSender::new(tx_raw);
    let parser = NutzapInfoArrivalParser {
        state: Arc::clone(&state),
        tx,
    };

    parser.parse_at(&verified_kind_10019(RECIPIENT), 200);

    let mut redriven_amounts = Vec::new();
    for _ in 0..2 {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(nmp_core::ActorMail::Command(ActorCommand::Protocol(cmd))) => {
                // Only the shape is observable from outside `send.rs` (the
                // `ProtocolCommand` trait object erases the concrete type) —
                // running it end-to-end is `send_nutzap_await_tests.rs`'s job.
                drop(cmd);
                redriven_amounts.push(());
            }
            other => panic!("expected a redriven Protocol(SendNutzapCommand), got {other:?}"),
        }
    }
    assert_eq!(redriven_amounts.len(), 2, "both parked sends must redrive");
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "nothing else should have been sent"
    );

    // Each parked entry fires exactly once — a second arrival redrives
    // nothing (already taken).
    let (tx2_raw, rx2) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let parser2 = NutzapInfoArrivalParser {
        state: Arc::clone(&state),
        tx: CommandSender::new(tx2_raw),
    };
    parser2.parse_at(&verified_kind_10019(RECIPIENT), 201);
    assert!(
        rx2.recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "an already-redriven await must never fire twice"
    );
}

#[test]
fn arrival_from_an_unrelated_pubkey_redrives_nothing() {
    let state = Arc::new(Mutex::new(CashuWalletState::new()));
    lock_state(&state).park_send_await(RECIPIENT, "acct".to_string(), 21, None, None, 100);
    let (tx_raw, rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let parser = NutzapInfoArrivalParser {
        state: Arc::clone(&state),
        tx: CommandSender::new(tx_raw),
    };

    parser.parse_at(&verified_kind_10019(OTHER_PUBKEY), 200);

    assert!(rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    assert_eq!(
        lock_state(&state).take_send_awaits(RECIPIENT).len(),
        1,
        "the unrelated arrival must not have consumed the parked await"
    );
}

#[test]
fn ttl_sweep_fails_closed_only_entries_past_the_bound() {
    let state = Arc::new(Mutex::new(CashuWalletState::new()));
    {
        let mut s = lock_state(&state);
        // Parked at t=100, "now" will be 100 + TIMEOUT + 1 -> expired.
        s.park_send_await(
            RECIPIENT,
            "acct".to_string(),
            21,
            None,
            Some("cid-expired".to_string()),
            100,
        );
        // Parked just now -> not expired.
        s.park_send_await(
            FRESH_RECIPIENT,
            "acct".to_string(),
            9,
            None,
            Some("cid-fresh".to_string()),
            100 + NUTZAP_INFO_AWAIT_TIMEOUT_SECS,
        );
    }
    let (tx_raw, rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let tx = CommandSender::new(tx_raw);

    run_ttl_sweep(&state, &tx, 100 + NUTZAP_INFO_AWAIT_TIMEOUT_SECS + 1);

    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(nmp_core::ActorMail::Command(ActorCommand::ShowErrorToken { token })) => {
            assert_eq!(token.code(), ui_codes::NO_RECIPIENT_NUTZAP_INFO);
        }
        other => panic!("expected ShowErrorToken(NO_RECIPIENT_NUTZAP_INFO), got {other:?}"),
    }
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(nmp_core::ActorMail::Command(ActorCommand::ActionLedger(
            nmp_core::actor::ActionLedgerCommand::RecordFailure { correlation_id, .. },
        ))) => {
            assert_eq!(correlation_id, "cid-expired");
        }
        other => panic!("expected RecordFailure(cid-expired), got {other:?}"),
    }
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "the not-yet-expired await must not be touched"
    );
    assert_eq!(
        lock_state(&state).take_send_awaits(FRESH_RECIPIENT).len(),
        1,
        "the fresh await must still be parked"
    );
}
