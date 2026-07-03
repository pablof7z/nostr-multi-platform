//! Money-safety: a `CompleteDepositCashu` retry after the encrypt/sign/publish
//! chain fails must resume from already-minted proofs, never re-touch the
//! mint (a Cashu mint permanently refuses to mint an already-`ISSUED` quote —
//! see `PendingDeposit::minted_proofs`'s doc comment). This covers only
//! in-process, still-running failures (a port hiccup, a dead-but-alive actor
//! inbox) — `minted_proofs` is in-memory, so a hard process crash in that
//! window is tracked as issue #2910, not something this test proves.
//!
//! #2910/#2923 — the SAME money-safety concern one step later: a retry after
//! the token event was already SIGNED (but the publish right after failed,
//! e.g. #2923's "no relay resolves") must resume from the cached signed
//! event — never re-mint AND never re-sign — see
//! `PendingDeposit::signed_token`'s doc comment.

use super::*;
use nmp_core::actor::{PublishCommand, SignCommand};
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
                signed_token: None,
                chain_started_at: None,
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
                signed_token: None,
                chain_started_at: None,
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

/// A `PendingDeposit` whose token event was already SIGNED by a prior
/// attempt (the #2923 repro: signing succeeds, the publish right after it
/// fails — e.g. "no relay resolves") must resume by republishing that EXACT
/// cached event — never re-touch the mint, never re-run the encrypt/sign
/// chain (which would sign a second, differently-id'd event over the same
/// proofs and double-fold the ledger). Proved here the same way
/// `resumes_from_already_minted_proofs_without_re_touching_the_mint` proves
/// its own claim: a bound-then-dropped mint listener would surface as
/// `MINT_QUOTE_FAILED` if touched at all, and the FIRST command out of the
/// worker is `Publish(SignedEvent)` carrying the cached id — no `Sign`/
/// `Nip44Encrypt` step precedes it.
#[test]
fn resumes_from_a_signed_token_and_republishes_without_re_minting_or_re_signing() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-resign");
    let account = "dd".repeat(32);
    let signed = nmp_signer_iface::SignedEvent {
        id: "a".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account.clone(),
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "already-signed-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    };
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
            "quote-resign".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                // `still_held` compares against this — the exact proofs
                // `signed` was built from — to decide whether to republish.
                minted_proofs: Some(vec![synthetic_proof(15, "02dd")]),
                signed_token: Some(signed.clone()),
                chain_started_at: None,
            },
        );
        // Still held — the deposit's proofs were never spent, so a retry
        // must republish rather than treat it as stale.
        state.add_proofs(
            Some(signed.id.clone()),
            MINT.to_string(),
            vec![synthetic_proof(15, "02dd")],
        );
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_mint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        quote_id: "quote-resign".to_string(),
        mint: dead_mint,
        amount_sats: 15,
        account_pubkey: account,
        correlation_id: Some("cid-resign".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
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
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, .. }) => {
            assert_eq!(
                raw.id, signed.id,
                "must republish the SAME cached event, not a new one"
            );
        }
        other => panic!(
            "expected to resume straight into republish (no mint, no re-sign), got {other:?}"
        ),
    }

    // The entry stays retained — still no ACK loop to know it is finally
    // safe to forget (see `PendingDeposit::signed_token`'s doc comment).
    let state = state::lock_state(&backend.state);
    assert!(state.pending_deposits.contains_key("quote-resign"));
}

/// A signed-but-unpublished token event whose proofs have SINCE been spent
/// (e.g. sent as a nutzap) must not be blindly republished — that would
/// resurrect a stale, already-superseded event. The worker must not touch
/// the mint, must not emit a `Publish` command, and must report the deposit
/// as already settled instead.
#[test]
fn a_signed_token_whose_proofs_were_already_spent_is_not_republished() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-spent");
    let account = "dd".repeat(32);
    let signed = nmp_signer_iface::SignedEvent {
        id: "b".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account.clone(),
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "already-signed-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    };
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
            "quote-spent".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                minted_proofs: Some(vec![synthetic_proof(15, "02ee")]),
                signed_token: Some(signed.clone()),
                chain_started_at: None,
            },
        );
        // Deliberately no `add_proofs` for `signed.id` — these proofs have
        // already been removed (spent), so `guard.proofs` holds none of
        // them.
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_mint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id,
        quote_id: "quote-spent".to_string(),
        mint: dead_mint,
        amount_sats: 15,
        account_pubkey: account,
        correlation_id: Some("cid-spent".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
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
        ActorCommand::ActionLedger(nmp_core::actor::ActionLedgerCommand::RecordSuccess {
            correlation_id,
            ..
        }) => {
            assert_eq!(correlation_id, "cid-spent");
        }
        other => panic!("expected a benign RecordSuccess, got {other:?}"),
    }
}

/// A signed token event whose proofs are only PARTIALLY still held (some
/// spent, some not) must not be republished either — a partial spend already
/// makes the cached event's content stale (it claims proofs that no longer
/// exist), so `still_held` requires the FULL original proof set, not "at
/// least one."
#[test]
fn a_signed_token_partially_spent_is_not_republished() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-partial");
    let account = "dd".repeat(32);
    let signed = nmp_signer_iface::SignedEvent {
        id: "c".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account.clone(),
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "already-signed-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    };
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
            "quote-partial".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 15,
                // The original mint produced TWO proofs for this token event.
                minted_proofs: Some(vec![
                    synthetic_proof(10, "02ff"),
                    synthetic_proof(5, "0211"),
                ]),
                signed_token: Some(signed.clone()),
                chain_started_at: None,
            },
        );
        // Only ONE of the two survives (the other was spent) — still not a
        // fully-intact token, so it must not be republished.
        state.add_proofs(
            Some(signed.id.clone()),
            MINT.to_string(),
            vec![synthetic_proof(10, "02ff")],
        );
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_mint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id,
        quote_id: "quote-partial".to_string(),
        mint: dead_mint,
        amount_sats: 15,
        account_pubkey: account,
        correlation_id: Some("cid-partial".to_string()),
    });
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
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
        ActorCommand::ActionLedger(nmp_core::actor::ActionLedgerCommand::RecordSuccess {
            correlation_id,
            ..
        }) => {
            assert_eq!(correlation_id, "cid-partial");
        }
        other => panic!("expected a benign RecordSuccess (not a republish), got {other:?}"),
    }
}
