//! Money-safety: a `CompleteDepositCashu` retry after the encrypt/sign/publish
//! chain fails must resume from already-minted proofs, never re-touch the
//! mint (a Cashu mint permanently refuses to mint an already-`ISSUED` quote —
//! see `PendingDeposit::minted_proofs`'s doc comment). This covers only
//! in-process, still-running failures (a port hiccup, a dead-but-alive actor
//! inbox) — `minted_proofs` is in-memory, so a hard process crash in that
//! window is tracked as issue #2910, not something this test proves.

use super::*;
use nmp_core::actor::SignCommand;
use std::sync::Arc;

/// Seed a pending deposit whose proofs are ALREADY minted (as if a prior
/// `CompleteDepositCashu` attempt succeeded at `mint_tokens` but failed before the
/// chain finished publishing), then run `CashuCompleteDepositCommand` against
/// a mint URL nothing is listening on. If the worker touched the mint at all
/// it would report `MINT_QUOTE_FAILED` (connection refused) instead of
/// reaching the encrypt step — proving the retry skipped the mint entirely.
#[test]
fn resumes_from_already_minted_proofs_without_re_touching_the_mint() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-resume");
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
            "quote-resume".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: Some(vec![synthetic_proof(15, "02cc")]),
            },
        );
    }

    // A bound-then-dropped port refuses every connection — if the worker
    // tried any mint HTTP call at all, it would fail here.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_mint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        quote_id: "quote-resume".to_string(),
        mint: dead_mint,
        amount_sats: 15,
        account_pubkey: "ee".repeat(32),
        correlation_id: Some("cid-resume".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
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
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { .. }) => {}
        other => panic!(
            "expected to resume straight into the encrypt step (no mint HTTP), got {other:?}"
        ),
    }
}

/// A `MintSettled` operation whose keyset fetch or mint-tokens call fails
/// moves to `Unknown` (retryable), never `Failed` (terminal) — `Failed` has
/// no outgoing transitions in the saga's own state table, so a subsequent
/// successful retry would otherwise be unable to ever reach
/// `PublishPending`/`Settled` even though the deposit genuinely completed.
#[test]
fn a_retryable_mint_http_failure_marks_the_operation_unknown_not_failed() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-uncertain");
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
            "quote-uncertain".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: None,
            },
        );
    }
    let mock_url = spawn_mock_mint(vec![(
        200,
        serde_json::json!({
            "quote": "quote-uncertain",
            "request": "lnbc100n1testnut",
            "amount": 15,
            "unit": "sat",
            "state": "PAID",
            "expiry": null,
        })
        .to_string(),
    )]);
    // No further canned responses — the keyset fetch that follows hits a
    // closed connection and fails, exercising the `mark_operation_uncertain` path.
    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        quote_id: "quote-uncertain".to_string(),
        mint: mock_url,
        amount_sats: 15,
        account_pubkey: "ee".repeat(32),
        correlation_id: Some("cid-uncertain".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
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
            assert_eq!(token.code(), ui_codes::MINT_TOKENS_FAILED);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::Unknown,
        "a retryable mint HTTP failure must not leave the operation terminally Failed"
    );
}
