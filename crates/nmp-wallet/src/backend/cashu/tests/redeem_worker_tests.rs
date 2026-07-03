//! `redeem::finish_redeem` — the worker-thread half of `RedeemNutzap` (W9,
//! #2917), driven directly with synthetic post-swap proofs. Split out of
//! `redeem_tests.rs` (AGENTS.md file-size discipline) — that file owns the
//! `RedeemNutzapCommand::run` verification-gate tests, this one owns the
//! post-verification swap->publish->settle worker.

use super::*;
use nmp_core::actor::PublishCommand;
use nmp_nip60::kinds::{KIND_NIP60_HISTORY, KIND_NIP60_TOKEN};

// Each test submodule under `tests/` defines its own small fixtures rather
// than sharing across siblings (matches `redeem_tests.rs`/`send_tests.rs`'s
// own `ACCOUNT` constants) — this is that split's own copy, same value.
const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

/// `finish_redeem` driven directly with a synthetic post-swap fresh proof:
/// proves the MintSettled->PublishPending->Settled ordering, that the history
/// event carries a plain redeemed `e` tag and sender `p` tag, and that the
/// ledger only folds `TokenAdded`/`NutzapRedeemed` (i.e. only marks the
/// nutzap redeemed) once BOTH kind:7375 and kind:7376 are signed.
///
/// Message order note: `chain.rs`'s `on_signed` callback (here, the closure
/// that kicks off the kind:7376 chain) runs BEFORE the kind:7375
/// `Publish(SignedEvent)` command is enqueued (see `chain.rs`'s
/// `sign_and_publish`) — so the kind:7376 `Nip44EncryptForAccount` lands on
/// the channel ahead of kind:7375's own publish. This does not reorder any
/// EFFECT (kind:7376 still can't be signed until its own async continuation
/// fires later), only the two commands' relative enqueue order.
#[test]
fn finish_redeem_publishes_token_then_history_and_settles() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let sender_hex = sender.public_key().to_hex();
    let our_pubkey = "02".to_string() + &"aa".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey);
    let operation_id = crate::journal::WalletOperationId::new("cid-finish-redeem");
    let nutzap_event_id = "d".repeat(64);
    {
        let mut state = lock_state(&backend.state);
        state
            .begin_operation(operation_id.clone(), WalletOperationKind::RedeemNutzap)
            .unwrap();
        state
            .journal
            .record_consumed_input(
                &operation_id,
                crate::journal::WalletConsumedInput {
                    event_id: nutzap_event_id.clone(),
                    mint: MINT.to_string(),
                    unit: "sat".to_string(),
                    amount: 21,
                },
            )
            .unwrap();
        state
            .transition(&operation_id, crate::journal::WalletOperationState::MintPending)
            .unwrap();
    }
    let nutzap = nmp_nip60::nutzap::ReceivedNutZap {
        event_id: nostr::EventId::from_hex(&nutzap_event_id).unwrap(),
        sender_pubkey: sender.public_key(),
        proofs: Vec::new(),
        mint_url: MINT.to_string(),
        amount_sats: 21,
        comment: String::new(),
        zapped_event_id: None,
    };
    let fresh_proofs = vec![synthetic_proof(21, "02fresh")];

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    redeem::finish_redeem(redeem::FinishRedeemArgs {
        worker_tx: nmp_core::CommandSender::new(worker_tx),
        state: std::sync::Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        account_pubkey: ACCOUNT.to_string(),
        nutzap,
        nutzap_wallet_event: crate::journal::WalletEventId::new(nutzap_event_id.clone()),
        fresh_proofs,
        relays: vec!["wss://my-relay.example".to_string()],
        created_at: 1_700_000_000,
        correlation_id: Some("cid-finish-redeem".to_string()),
    });

    // kind:7375 (fresh proofs) — NIP-44 self-encrypted, no plain tags.
    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::Nip44EncryptForAccount {
            peer_pubkey,
            continuation,
            ..
        }) => {
            assert_eq!(peer_pubkey, ACCOUNT);
            continuation
        }
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-token-ciphertext".to_string()));
    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, KIND_NIP60_TOKEN);
            assert!(unsigned.tags.is_empty());
            continuation
        }
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    // Triggers `on_signed`, which kicks off the kind:7376 chain BEFORE the
    // kind:7375 `Publish` is enqueued — see this test's own doc comment.
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "f".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: KIND_NIP60_TOKEN,
            tags: Vec::new(),
            content: "fake-token-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    }));

    let history_encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::Nip44EncryptForAccount {
            continuation,
            ..
        }) => continuation,
        other => panic!("expected Nip44EncryptForAccount for history, got {other:?}"),
    };
    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, .. }) => {
            assert_eq!(raw.kind, KIND_NIP60_TOKEN);
        }
        other => panic!("expected Publish(SignedEvent) for kind:7375, got {other:?}"),
    }

    // Between the two publishes: PublishPending, not yet Settled, and the
    // nutzap must NOT be marked redeemed yet.
    {
        let state = lock_state(&backend.state);
        assert_eq!(
            state.journal.get(&operation_id).unwrap().state,
            crate::journal::WalletOperationState::PublishPending
        );
        assert!(!state
            .ledger
            .state()
            .is_nutzap_redeemed(&crate::journal::WalletEventId::new(nutzap_event_id.clone())));
    }

    // kind:7376 (history) — encrypted content, plain redeemed e + sender p tags.
    history_encrypt_continuation.call(Ok("fake-history-ciphertext".to_string()));
    let history_sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            unsigned,
            continuation,
            ..
        }) => {
            assert_eq!(unsigned.kind, KIND_NIP60_HISTORY);
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec![
                    "e".to_string(),
                    nutzap_event_id.clone(),
                    String::new(),
                    "redeemed".to_string()
                ]));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["p".to_string(), sender_hex.clone()]));
            continuation
        }
        other => panic!("expected EventForAccount for history, got {other:?}"),
    };
    history_sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "1".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: KIND_NIP60_HISTORY,
            tags: Vec::new(),
            content: "fake-history-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    }));
    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, .. }) => {
            assert_eq!(raw.kind, KIND_NIP60_HISTORY);
        }
        other => panic!("expected Publish(SignedEvent) for kind:7376, got {other:?}"),
    }

    // Only now: Settled, nutzap marked redeemed, fresh proof in inventory.
    let state = lock_state(&backend.state);
    assert_eq!(
        state.journal.get(&operation_id).unwrap().state,
        crate::journal::WalletOperationState::Settled
    );
    assert!(state
        .ledger
        .state()
        .is_nutzap_redeemed(&crate::journal::WalletEventId::new(nutzap_event_id)));
    assert!(state.proofs.iter().any(|p| p.proof.c == "02fresh"));
    assert_eq!(
        state.ledger.state().balance(
            &crate::journal::MintUrl::new(MINT),
            &crate::journal::WalletUnit::new("sat")
        ),
        21_000
    );
}
