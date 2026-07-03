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
