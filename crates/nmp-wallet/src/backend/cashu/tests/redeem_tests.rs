//! `RedeemNutzap` — W9 (#2917): independent verification gates
//! (`RedeemNutzapCommand::run`) and the swap-before-redeem fold + kind:7375/
//! kind:7376 publish (`redeem::finish_redeem`, driven directly with
//! synthetic proofs — the mint HTTP/DHKE round-trip itself is
//! `nmp-nip60`'s own tested surface).

use super::*;
use nmp_core::actor::PublishCommand;
use nmp_nip60::kinds::{KIND_NIP60_HISTORY, KIND_NIP60_TOKEN};
use nmp_nip60::nutzap::{p2pk_secret, NutZapProof};

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

fn run_redeem(
    backend: &CashuWalletBackend,
    cached: &FixedCachedEvents,
    correlation_id: &str,
    event_id: &str,
) -> RecordingErrorSurface {
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::RedeemNutzap {
            event_id: event_id.to_string(),
        },
        Some(correlation_id.to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://my-relay.example".to_string()]);
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    errors
}

fn expect_error_code(errors: &RecordingErrorSurface, code: &str) {
    assert_eq!(errors.last_token_code.lock().unwrap().as_deref(), Some(code));
}

/// A proof with a P2PK secret locked to `pubkey`, no DLEQ (mint doesn't
/// advertise NUT-12 — `verify_nutzap_dleq` skips proofs with no `dleq`, per
/// its own doc comment, so this is a valid "no DLEQ offered" fixture, not a
/// bypass).
fn locked_proof(amount: u64, c: &str, pubkey: &str) -> NutZapProof {
    NutZapProof {
        amount,
        id: "keyset-1".to_string(),
        secret: p2pk_secret(pubkey),
        c: c.to_string(),
        dleq: None,
    }
}

#[test]
fn unknown_event_id_fails_closed() {
    let backend = backend_with_mint();
    let errors = run_redeem(
        &backend,
        &FixedCachedEvents::default(),
        "cid-unknown",
        &"e".repeat(64),
    );
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

#[test]
fn nutzap_not_p_tagged_to_self_fails_closed() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let someone_else = "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(&sender, proofs, MINT, someone_else, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-not-self", &event_id);
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

#[test]
fn untrusted_mint_fails_closed() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"55".repeat(32));
    let sender = nostr::Keys::generate();
    let our_pubkey = "02".to_string() + &"55".repeat(32);
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(
        &sender,
        proofs,
        "https://untrusted-mint.example",
        ACCOUNT,
        None,
        None,
        1_699_999_500,
    );
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-untrusted-mint", &event_id);
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);
}

#[test]
fn no_cashu_wallet_fails_closed() {
    let backend = backend_with_mint(); // mint accepted, but no cashu_pubkey_hex set
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-no-wallet", &event_id);
    expect_error_code(&errors, ui_codes::NO_CASHU_WALLET);
}

#[test]
fn proof_not_locked_to_our_pubkey_fails_closed() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"66".repeat(32));
    let sender = nostr::Keys::generate();
    // Locked to a DIFFERENT pubkey than the one this wallet holds.
    let proofs = vec![locked_proof(21, "02aa", &("02".to_string() + &"77".repeat(32)))];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-wrong-lock", &event_id);
    expect_error_code(&errors, ui_codes::INVALID_NUTZAP);
}

/// A wallet whose `create` flow ran (so `cashu_pubkey_hex` is set) but whose
/// privkey never made it into THIS session's live state (the documented
/// cold-start-recovery deferral) must still fail closed rather than silently
/// skip the P2PK witness.
#[test]
fn missing_privkey_fails_closed() {
    let backend = backend_with_mint();
    let our_pubkey = "02".to_string() + &"88".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey.clone());
    // Deliberately no `cashu_privkey` set.
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-no-privkey", &event_id);
    expect_error_code(&errors, ui_codes::NO_CASHU_WALLET);
}

/// #2942 — the OBSERVER path (`CashuWalletBackend::on_wallet_event`, called
/// exactly this way by `WalletRuntime`'s `WalletEventSink::on_kernel_event` in
/// production — see `runtime.rs`) must dispatch the same `RedeemNutzapCommand`
/// the explicit `nmp.wallet.nutzap.redeem` action reaches, for an inbound
/// `#p`=self kind:9321 the runtime's reconciler observes. Before #2942's fix
/// this path never ran at all for a fresh account (the nutzap-receipts
/// interest opened at the wrong routing scope and got zero relay entries —
/// see `runtime.rs`'s module docs); this test proves that once the event DOES
/// reach the sink (live delivery, or a backfill/EOSE replay — both funnel
/// through the same `ObservedProjectionSink::on_kernel_event` call), a
/// deliberately-unverifiable nutzap (untrusted mint) lands at `Failed` in the
/// journal — visibly rejected, never silently absent — matching the design
/// doc's "Observer counting: unverifiable nutzaps may be shown as
/// rejected/ignored state, not counted as value"
/// (`docs/architecture/nip60-nip61-wallet-design.md`).
#[test]
fn on_wallet_event_dispatches_redeem_and_a_deliberately_unverifiable_nutzap_lands_failed() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", "unused-lock-target")];
    let event = nutzap_kernel_event(
        &sender,
        proofs,
        "https://untrusted-mint.example",
        ACCOUNT,
        None,
        None,
        1_699_999_500,
    );
    let event_id = event.id.clone();

    // The exact call `WalletEventSink::on_kernel_event` makes in production
    // (runtime.rs) — NOT `start_intent(RedeemNutzap)`, which is the
    // explicit-action path the other tests in this file exercise.
    let commands = backend.on_wallet_event(ctx(Some(ACCOUNT)), &event);
    let ActorCommand::Protocol(cmd) = commands
        .into_iter()
        .next()
        .expect("on_wallet_event must dispatch a RedeemNutzapCommand for a #p=self kind:9321")
    else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://my-relay.example".to_string()]);
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let cached = FixedCachedEvents(vec![event]);
    {
        let mut c = ctx_with_cached_events_and_errors(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached,
            &errors,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);

    let operation_id = super::super::nutzap_dispatch::redeem_operation_id(&event_id);
    let state = lock_state(&backend.state);
    let operation = state.journal.get(&operation_id).expect(
        "the observer-dispatched operation must be recorded in the journal — never \
         silently absent, even for a rejected nutzap",
    );
    assert_eq!(
        operation.state,
        crate::journal::WalletOperationState::Failed,
        "a deliberately-unverifiable (untrusted mint) nutzap must land Failed, not vanish"
    );
}

/// Redeeming the SAME event twice — once already folded as `NutzapRedeemed`
/// — must fail closed on the second attempt rather than double-count.
#[test]
fn already_redeemed_fails_closed() {
    let backend = backend_with_mint();
    let our_pubkey = "02".to_string() + &"99".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey.clone());
    let sender = nostr::Keys::generate();
    let proofs = vec![locked_proof(21, "02aa", &our_pubkey)];
    let event = nutzap_kernel_event(&sender, proofs, MINT, ACCOUNT, None, None, 1_699_999_500);
    let event_id = event.id.clone();
    {
        let mut state = lock_state(&backend.state);
        state.ledger.apply(crate::journal::WalletFact::NutzapRedeemed {
            nutzap: crate::journal::WalletEventId::new(event_id.clone()),
            amount_msat: 21_000,
            sender: crate::journal::PubkeyRef::new(sender.public_key().to_hex()),
        });
    }
    let cached = FixedCachedEvents(vec![event]);
    let errors = run_redeem(&backend, &cached, "cid-already", &event_id);
    expect_error_code(&errors, ui_codes::ALREADY_REDEEMED);
}

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
