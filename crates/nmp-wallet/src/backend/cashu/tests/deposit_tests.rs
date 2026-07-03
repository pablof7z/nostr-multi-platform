//! `DepositQuoteCashu` / `CompleteDepositCashu` — journal ordering, the auto-settle path
//! against a mockable mint, fail-closed gates, and `dispatch_token_event`'s
//! ledger/journal wiring with synthetic proofs.

use super::*;
use nmp_core::actor::{ActionLedgerCommand, PublishCommand, SignCommand};
use std::sync::Arc;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

// ── DepositQuoteCashu: fail-closed gates + journal ordering ─────────────────────

#[test]
fn deposit_quote_rejects_a_mint_this_wallet_was_not_created_with() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::DepositQuoteCashu {
            mint: "https://other-mint.example".to_string(),
            amount_sats: 21,
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::UNSUPPORTED_MINT)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn deposit_quote_rejects_zero_amount() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::DepositQuoteCashu {
            mint: MINT.to_string(),
            amount_sats: 0,
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::UNSUPPORTED_MINT)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

/// The core saga invariant: the journal already records this operation as
/// `MintPending` synchronously inside `start_intent`, before the returned
/// `Protocol` command has even been run — i.e. before any mint HTTP request
/// exists. No network access needed to prove this.
#[test]
fn deposit_quote_journals_mint_pending_before_the_mint_request() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::DepositQuoteCashu {
            mint: MINT.to_string(),
            amount_sats: 21,
        },
        Some("cid-quote".to_string()),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::Protocol(_)));

    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-quote"))
        .expect("operation recorded before any HTTP call");
    assert_eq!(op.state, crate::journal::WalletOperationState::MintPending);
    assert_eq!(op.kind, WalletOperationKind::DepositCashu);
}

// ── DepositQuoteCashu: the auto-settle path against a mockable mint ─────────────

/// testnut-style: the quote request round-trips against a mock mint and the
/// wiring records the quote as pending + surfaces `{quote_id, bolt11}`
/// through the action-result channel — the same shape a real caller uses to
/// pay the invoice and later call `CompleteDepositCashu`.
#[test]
fn deposit_quote_completes_against_a_mock_mint() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::DepositQuoteCashu {
            mint: MINT.to_string(),
            amount_sats: 100,
        },
        Some("cid-mock-quote".to_string()),
    );
    let ActorCommand::Protocol(_) = &commands[0] else {
        panic!("expected a Protocol command");
    };

    // Redirect at the `MintClient` level: build the command directly against
    // the mock mint's URL rather than the intent's `mint` field, since
    // `start_intent` already validated against the wallet's accepted list —
    // the command itself is what actually talks HTTP.
    let mock_url = spawn_mock_mint(vec![(
        200,
        serde_json::json!({
            "quote": "quote-abc",
            "request": "lnbc100n1testnut",
            "amount": 100,
            "unit": "sat",
            "state": "UNPAID",
            "expiry": null,
        })
        .to_string(),
    )]);

    let operation_id = crate::journal::WalletOperationId::new("cid-mock-quote");
    let cmd = Box::new(deposit::CashuDepositQuoteCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        mint: mock_url,
        amount_sats: 100,
        correlation_id: Some("cid-mock-quote".to_string()),
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
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess {
            correlation_id,
            result_json,
        }) => {
            assert_eq!(correlation_id, "cid-mock-quote");
            let json: serde_json::Value = serde_json::from_str(&result_json.unwrap()).unwrap();
            assert_eq!(json["quote_id"], "quote-abc");
            assert_eq!(json["bolt11"], "lnbc100n1testnut");
        }
        other => panic!("expected RecordActionSuccess, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    let pending = state
        .pending_deposits
        .get("quote-abc")
        .expect("quote recorded as pending");
    assert_eq!(pending.amount_sats, 100);
    let op = state.journal.get(&operation_id).unwrap();
    assert_eq!(op.state, crate::journal::WalletOperationState::MintSettled);
}

#[test]
fn deposit_quote_fails_closed_when_the_mint_is_unreachable() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-unreachable");
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
    }
    // A closed listener (bound-then-dropped port) refuses the connection —
    // no live network dependency needed to exercise the failure branch.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let cmd = Box::new(deposit::CashuDepositQuoteCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        mint: format!("http://{addr}"),
        amount_sats: 21,
        correlation_id: Some("cid-unreachable".to_string()),
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
            assert_eq!(token.code(), ui_codes::MINT_QUOTE_FAILED)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::Failed
    );
}

// ── CompleteDepositCashu: fail-closed gates ──────────────────────────────────────

#[test]
fn complete_deposit_rejects_an_unknown_quote_id() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::CompleteDepositCashu {
            quote_id: "never-seen".to_string(),
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => assert_eq!(token.code(), ui_codes::UNKNOWN_QUOTE),
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn complete_deposit_requires_an_active_account() {
    let backend = backend_with_mint();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::CompleteDepositCashu {
            quote_id: "whatever".to_string(),
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => assert_eq!(token.code(), ui_codes::NO_ACCOUNT),
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

/// testnut-style: the quote-status check reports `UNPAID` — retryable, not a
/// hard journal failure.
#[test]
fn complete_deposit_reports_not_yet_paid_without_failing_the_operation() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-notpaid");
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
    }
    let mock_url = spawn_mock_mint(vec![(
        200,
        serde_json::json!({
            "quote": "quote-notpaid",
            "request": "lnbc100n1testnut",
            "amount": 21,
            "unit": "sat",
            "state": "UNPAID",
            "expiry": null,
        })
        .to_string(),
    )]);
    let cmd = Box::new(deposit::CashuCompleteDepositCommand {
        state: Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        quote_id: "quote-notpaid".to_string(),
        mint: mock_url,
        amount_sats: 21,
        account_pubkey: "aa".repeat(32),
        correlation_id: Some("cid-notpaid".to_string()),
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
            assert_eq!(token.code(), ui_codes::QUOTE_NOT_PAID)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::MintSettled,
        "not-yet-paid must not fail the operation — it stays retryable"
    );
}

// ── dispatch_token_event: ledger/journal wiring with synthetic proofs ──────

/// Drives the post-mint wiring directly with synthetic proofs — the DHKE
/// unblind+verify math itself is `nmp-nip60`'s own tested surface (this
/// function only calls it); what this test proves is the ledger fact +
/// journal transition + pending-deposit cleanup this crate owns.
#[test]
fn dispatch_token_event_applies_token_added_and_settles_the_operation() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-complete");
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
            "quote-xyz".to_string(),
            state::PendingDeposit {
                operation_id: operation_id.clone(),
                mint: MINT.to_string(),
                amount_sats: 30,
                minted_proofs: None,
            },
        );
    }

    let account = "dd".repeat(32);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();

    // No `ProtocolCommandContext` needed here — `dispatch_token_event` only
    // holds an owned `CommandSender`, mirroring the worker-thread shape it
    // actually runs under in production.
    deposit::dispatch_token_event(
        nmp_core::CommandSender::new(worker_tx),
        Arc::clone(&backend.state),
        operation_id.clone(),
        "quote-xyz".to_string(),
        MINT.to_string(),
        vec![synthetic_proof(20, "02aa"), synthetic_proof(10, "02bb")],
        account.clone(),
        vec!["wss://relay.example".to_string()],
        1_700_000_000,
        Some("cid-complete".to_string()),
    );

    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
            peer_pubkey,
            continuation,
            ..
        }) => {
            assert_eq!(peer_pubkey, account);
            continuation
        }
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-token-ciphertext".to_string()));

    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, nmp_nip60::KIND_NIP60_TOKEN);
            assert_eq!(
                unsigned.created_at, 1_700_000_000,
                "created_at must be the caller-supplied wall-clock value, never a 0 sentinel"
            );
            continuation
        }
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    let signed = nmp_signer_iface::SignedEvent {
        id: "f".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account,
            kind: nmp_nip60::KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "fake-token-ciphertext".to_string(),
            created_at: 0,
        },
    };
    sign_continuation.call(Ok(signed));

    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, .. }) => {
            assert_eq!(raw.kind, nmp_nip60::KIND_NIP60_TOKEN);
        }
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(MINT),
            &crate::journal::WalletUnit::new("sat")
        ),
        30_000,
        "amount_msat = sats*1000"
    );
    assert!(
        !state.pending_deposits.contains_key("quote-xyz"),
        "the completed quote is no longer pending"
    );
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::PublishPending
    );
}
