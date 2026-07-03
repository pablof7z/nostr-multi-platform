//! Money-safety concurrency: the `chain_started_at` lease that stops two
//! `CompleteDepositCashu` attempts for the SAME `quote_id` from each signing a
//! separate token event over the same real proofs and double-folding the
//! ledger. Split out of `deposit_retry_tests.rs` (AGENTS.md LOC discipline) —
//! that file owns the sequential resume/idempotency cases (minted-proofs and
//! signed-token resume), this one owns the concurrent-attempt lease + fencing
//! cases:
//!
//! - a concurrent attempt inside the lease window is rejected, not
//!   double-dispatched;
//! - once the lease expires the previous attempt is presumed abandoned and a
//!   new one may take over;
//! - a stale attempt that reaches `on_signed` after a newer one took over is
//!   fenced out and must NOT fold its proofs/ledger fact a second time.

use super::*;
use nmp_core::actor::{PublishCommand, SignCommand};
use std::sync::Arc;

/// Money-safety concurrency guard: a `CompleteDepositCashu` attempt for a
/// `quote_id` whose chain is already in flight (a prior attempt's lease
/// hasn't expired yet) must be rejected as retryable rather than launching a
/// SECOND encrypt/sign chain over the same minted proofs — two signed token
/// events for one real deposit would double-fold the ledger.
#[test]
fn a_concurrent_attempt_within_the_lease_window_is_rejected_not_double_dispatched() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-inflight");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintPending,
            )
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintSettled,
            )
            .unwrap();
        state.pending_deposits.insert(
            "quote-inflight".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: Some(vec![synthetic_proof(15, "0299")]),
                signed_token: None,
                // A prior attempt started 10s ago — well within the lease.
                chain_started_at: Some(10),
            },
        );
    }

    // A bound-then-dropped port refuses every connection — if this attempt
    // touched the mint (or resumed the chain) at all, it would show up here.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_mint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id,
        quote_id: "quote-inflight".to_string(),
        mint: dead_mint,
        amount_sats: 15,
        account_pubkey: "ee".repeat(32),
        correlation_id: Some("cid-inflight".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    // Only 30s after the lease started — still well inside the 60s window.
    let clock = FixedClock(40);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    match recv_command(&worker_rx) {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::DEPOSIT_IN_PROGRESS);
        }
        other => panic!("expected DEPOSIT_IN_PROGRESS, got {other:?}"),
    }
}

/// The other half of the lease: once it expires, the previous attempt is
/// presumed abandoned and a new one may take over (never permanently
/// stranding a deposit just because one attempt's chain never reported
/// back).
#[test]
fn an_expired_lease_lets_a_new_attempt_take_over() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-expired");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintPending,
            )
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintSettled,
            )
            .unwrap();
        state.pending_deposits.insert(
            "quote-expired".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: Some(0),
            },
        );
    }

    let mock_url = spawn_mock_mint(vec![(
        200,
        serde_json::json!({
            "quote": "quote-expired",
            "request": "lnbc100n1testnut",
            "amount": 15,
            "unit": "sat",
            "state": "UNPAID",
            "expiry": null,
        })
        .to_string(),
    )]);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id,
        quote_id: "quote-expired".to_string(),
        mint: mock_url,
        amount_sats: 15,
        account_pubkey: "ee".repeat(32),
        correlation_id: Some("cid-expired".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    // Well past the 60s lease from `chain_started_at: Some(0)`.
    let clock = FixedClock(120);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    // Reached the mock mint (not blocked as in-progress) and got the
    // expected not-yet-paid, retryable response.
    match recv_command(&worker_rx) {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::QUOTE_NOT_PAID);
        }
        other => panic!("expected the lease to have expired and the mint to be reachable, got {other:?}"),
    }
}

/// Money-safety fencing: if a NEWER attempt has taken over a quote's lease
/// (this one only got there because the previous one's lease expired — see
/// `an_expired_lease_lets_a_new_attempt_take_over`) between when a STALE
/// attempt's chain started and when it finally reaches `on_signed`, the
/// stale attempt's `on_signed` must NOT apply its proofs/ledger fact —
/// otherwise the SAME real proofs would fold into the ledger/spendable
/// inventory twice (once per attempt's differently-id'd token event).
/// Exercises `dispatch_token_event` directly (mirrors
/// `dispatch_token_event_applies_token_added_and_settles_the_operation`'s
/// shape) with a `chain_started_at` that does NOT match the `created_at`
/// this call is stamped with, simulating exactly that race.
#[test]
fn on_signed_skips_the_ledger_fold_when_a_newer_attempt_has_taken_over() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-stale");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintPending,
            )
            .unwrap();
        state
            .transition(
                &operation_id,
                crate::journal::WalletOperationState::MintSettled,
            )
            .unwrap();
        state.pending_deposits.insert(
            "quote-stale".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 30,
                minted_proofs: None,
                signed_token: None,
                // A DIFFERENT (newer) attempt's lease — not this call's own
                // `created_at` (1_700_000_000) below — as if attempt B took
                // over after this (attempt A's) lease expired.
                chain_started_at: Some(1_700_000_999),
            },
        );
    }

    let account = "dd".repeat(32);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    deposit::dispatch_token_event(
        nmp_core::CommandSender::new(worker_tx),
        Arc::clone(&backend.state),
        operation_id.clone(),
        "quote-stale".to_string(),
        MINT.to_string(),
        vec![synthetic_proof(30, "02cd")],
        account.clone(),
        vec!["wss://relay.example".to_string()],
        1_700_000_000,
        Some("cid-stale".to_string()),
    );

    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { continuation, .. }) => {
            continuation
        }
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-ciphertext".to_string()));
    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount { continuation, .. }) => continuation,
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    let signed = nmp_signer_iface::SignedEvent {
        id: "d".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account,
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "fake-ciphertext".to_string(),
            created_at: 0,
        },
    };
    sign_continuation.call(Ok(signed));

    // The chain still publishes (chain.rs has no way to abort that from
    // inside `on_signed`) — this test's point is what `on_signed` itself
    // did NOT do to shared wallet state.
    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { .. }) => {}
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(MINT),
            &crate::journal::WalletUnit::new("sat")
        ),
        0,
        "a fenced-out (superseded) attempt must not fold TokenAdded"
    );
    assert!(
        state.proofs.is_empty(),
        "a fenced-out attempt must not add its proofs to the spendable inventory either"
    );
    // The newer attempt's lease is untouched by the stale one's `on_signed`.
    assert_eq!(
        state
            .pending_deposits
            .get("quote-stale")
            .unwrap()
            .chain_started_at,
        Some(1_700_000_999)
    );
}
