//! `send::finish_send` — the worker-thread half of `SendNutzap` (W8, #2917),
//! driven directly with synthetic post-swap proofs. Split out of
//! `send_tests.rs` (AGENTS.md file-size discipline) — that file owns the
//! `SendNutzapCommand::run` verification-gate tests, this one owns the
//! post-swap fold->publish worker (mirrors `redeem_tests.rs`/
//! `redeem_worker_tests.rs`'s identical split).

use super::*;
use nmp_core::actor::PublishCommand;
use nmp_core::publish::PublishTarget;
use nmp_nip60::kinds::KIND_NIP61_NUTZAP;

// Each test submodule under `tests/` defines its own small fixtures rather
// than sharing across siblings (matches `redeem_worker_tests.rs`'s own
// `ACCOUNT` constant) — this is that split's own copy, same values
// `send_tests.rs` uses.
const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

/// `finish_send` driven directly with synthetic post-swap proofs (mirrors
/// `dispatch_token_event_applies_token_added_and_settles_the_operation` in
/// `deposit_tests`): proves the spent-proof removal, change-proof addition,
/// ledger fold, kind:9321 tag content, and terminal journal state — none of
/// which needs a live mint.
#[test]
fn finish_send_updates_proofs_ledger_and_publishes_kind_9321() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-finish-send");
    let spent_proof = synthetic_proof(30, "02aa");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::SendNutzap)
            .unwrap();
        state.add_proofs(
            Some("token-event-1".to_string()),
            MINT.to_string(),
            vec![spent_proof.clone()],
        );
        state
            .journal
            .record_consumed_input(
                &operation_id,
                crate::journal::WalletConsumedInput {
                    event_id: "token-event-1".to_string(),
                    mint: MINT.to_string(),
                    unit: "sat".to_string(),
                    amount: 30,
                },
            )
            .unwrap();
        state
            .transition(&operation_id, crate::journal::WalletOperationState::MintPending)
            .unwrap();
    }
    let selected = vec![state::StoredProof {
        token_event: Some("token-event-1".to_string()),
        mint: MINT.to_string(),
        proof: spent_proof,
    }];
    // `finish_send` no longer removes `selected` itself — its caller
    // (`SendNutzapCommand::run`) reserves the selection synchronously
    // BEFORE spawning the worker (closes the double-tap race; see that
    // call site's comment), so a direct `finish_send` test must mirror
    // that reservation itself.
    state::lock_state(&backend.state).remove_proofs(&selected);
    // 21 sats P2PK-locked to the recipient + 8 sats change (no fee — a
    // synthetic swap response, never a real mint's arithmetic).
    let new_proofs = vec![synthetic_proof(21, "02bb"), synthetic_proof(8, "02cc")];

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    send::finish_send(send::FinishSendArgs {
        worker_tx: nmp_core::CommandSender::new(worker_tx),
        state: std::sync::Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        account_pubkey: ACCOUNT.to_string(),
        recipient_pubkey: RECIPIENT.to_string(),
        mint: MINT.to_string(),
        selected,
        new_proofs,
        nutzap_count: 1,
        target_event_id: None,
        relays: vec!["wss://recipient-relay.example".to_string()],
        created_at: 1_700_000_000,
        correlation_id: Some("cid-finish-send".to_string()),
    });

    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            signer_pubkey,
            continuation,
        }) => {
            assert_eq!(unsigned.kind, KIND_NIP61_NUTZAP);
            assert_eq!(signer_pubkey.as_deref(), Some(ACCOUNT));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["u".to_string(), MINT.to_string()]));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["p".to_string(), RECIPIENT.to_string()]));
            assert!(
                unsigned.tags.iter().any(|t| t.first().map(String::as_str) == Some("proof")),
                "expected at least one proof tag, got {:?}",
                unsigned.tags
            );
            continuation
        }
        other => panic!("expected EventForAccount (no nip44 step), got {other:?}"),
    };
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "e".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: KIND_NIP61_NUTZAP,
            tags: Vec::new(),
            content: String::new(),
            created_at: 1_700_000_000,
        },
    }));

    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, target, .. }) => {
            assert_eq!(raw.kind, KIND_NIP61_NUTZAP);
            match target {
                PublishTarget::Explicit { relays, .. } => {
                    assert_eq!(relays, vec!["wss://recipient-relay.example".to_string()]);
                }
                other => panic!("expected explicit publish target, got {other:?}"),
            }
        }
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::PublishPending
    );
    // The spent proof (c = "02aa") is gone; the change proof (c = "02cc") is
    // held. The nutzap's own output proof (c = "02bb") was handed to the
    // recipient, never kept locally.
    assert!(!state.proofs.iter().any(|p| p.proof.c == "02aa"));
    assert!(state.proofs.iter().any(|p| p.proof.c == "02cc"));
    assert!(!state.proofs.iter().any(|p| p.proof.c == "02bb"));
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(MINT),
            &crate::journal::WalletUnit::new("sat")
        ),
        8_000,
        "only the 8-sat change proof remains in balance (amount_msat = sats*1000)"
    );
}

/// #2934 money-safety fix — a sign failure AFTER the mint swap has already
/// committed (mirrors a NIP-46 bunker timeout/disconnect) must never debit
/// the sender's reported balance for a nutzap that was never actually
/// signed or published. Same setup as
/// `finish_send_updates_proofs_ledger_and_publishes_kind_9321` up to the
/// sign step, but fails the sign instead of completing it — proving the
/// ledger fold (and the operation's journal transition) only ever runs
/// inside a successful `on_signed`, never before it.
#[test]
fn finish_send_sign_failure_leaves_balance_and_journal_state_untouched() {
    let backend = backend_with_mint();
    let operation_id = crate::journal::WalletOperationId::new("cid-sign-fail");
    let spent_proof = synthetic_proof(30, "02aa");
    {
        let mut state = state::lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::SendNutzap)
            .unwrap();
        state.add_proofs(
            Some("token-event-1".to_string()),
            MINT.to_string(),
            vec![spent_proof.clone()],
        );
        // Seed the pre-send ledger balance the same way a real prior deposit
        // fold would have, so the assertion below proves the balance is
        // UNCHANGED after the failed sign, not merely absent.
        state.ledger.apply(crate::journal::WalletFact::TokenAdded {
            token_event: crate::journal::WalletEventId::new("token-event-1"),
            mint: crate::journal::MintUrl::new(MINT),
            unit: crate::journal::WalletUnit::new("sat"),
            proofs: vec![crate::journal::ProofAtom {
                proof: crate::journal::ProofRef::new("02aa".to_string()),
                amount_msat: 30_000,
            }],
            via: crate::journal::Provenance::Saga(crate::journal::CorrelationId::new("seed")),
        });
        state
            .journal
            .record_consumed_input(
                &operation_id,
                crate::journal::WalletConsumedInput {
                    event_id: "token-event-1".to_string(),
                    mint: MINT.to_string(),
                    unit: "sat".to_string(),
                    amount: 30,
                },
            )
            .unwrap();
        state
            .transition(&operation_id, crate::journal::WalletOperationState::MintPending)
            .unwrap();
    }
    let selected = vec![state::StoredProof {
        token_event: Some("token-event-1".to_string()),
        mint: MINT.to_string(),
        proof: spent_proof,
    }];
    state::lock_state(&backend.state).remove_proofs(&selected);
    let new_proofs = vec![synthetic_proof(21, "02bb"), synthetic_proof(8, "02cc")];

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    send::finish_send(send::FinishSendArgs {
        worker_tx: nmp_core::CommandSender::new(worker_tx),
        state: std::sync::Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        account_pubkey: ACCOUNT.to_string(),
        recipient_pubkey: RECIPIENT.to_string(),
        mint: MINT.to_string(),
        selected,
        new_proofs,
        nutzap_count: 1,
        target_event_id: None,
        relays: vec!["wss://recipient-relay.example".to_string()],
        created_at: 1_700_000_000,
        correlation_id: Some("cid-sign-fail".to_string()),
    });

    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount { continuation, .. }) => {
            continuation
        }
        other => panic!("expected EventForAccount (sign step), got {other:?}"),
    };
    // Simulate a NIP-46 bunker timeout/disconnect — `chain.rs`'s
    // `sign_and_publish` returns on a sign `Err` WITHOUT ever calling
    // `on_signed`.
    sign_continuation.call(Err("bunker timeout".to_string()));

    // `report_chain_failure` still surfaces an error token/action-failure
    // record on this same channel — drain whatever it sent and prove NONE of
    // it is a publish. The kind:9321 was never signed, so it must never
    // reach the publish pipeline either.
    while let Ok(mail) = worker_rx.recv_timeout(std::time::Duration::from_millis(50)) {
        let cmd = unwrap_mail(mail);
        assert!(
            !matches!(cmd, ActorCommand::Publish(_)),
            "a failed sign must never enqueue a publish, got {cmd:?}"
        );
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(MINT),
            &crate::journal::WalletUnit::new("sat")
        ),
        30_000,
        "the sender's reported balance must stay at its pre-send value — a \
         failed sign must never debit it"
    );
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::MintPending,
        "a failed sign must never advance the operation past MintPending \
         (MintSettled/PublishPending only ever follow a successful sign)"
    );
}
