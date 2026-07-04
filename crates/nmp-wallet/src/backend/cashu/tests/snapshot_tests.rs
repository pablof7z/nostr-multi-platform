//! `snapshot()` — the bounded `WalletProjection` never carries a secret or a
//! quote id, mirroring `projection.rs`'s own
//! `projection_never_requires_secret_wallet_material` test. #2949's
//! `recent_history`/`receive_rows` derivation (`snapshot.rs`) gets its own
//! two tests below: a settled redeem must surface an accepted receive row
//! plus a history row, and a deliberately-unverifiable redeem must surface a
//! rejected receive row rather than vanish once it goes terminal.

use nmp_core::actor::PublishCommand;
use nmp_nip60::kinds::{KIND_NIP60_HISTORY, KIND_NIP60_TOKEN};
use nmp_nip60::nutzap::{p2pk_secret, NutZapProof};

use super::*;

#[test]
fn capabilities_and_default_snapshot_are_not_configured() {
    let backend = CashuWalletBackend::new();
    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(
        snapshot.projection.readiness,
        WalletReadiness::NotConfigured
    );
    assert!(snapshot.projection.capabilities.create_cashu_wallet);
    assert!(snapshot.projection.capabilities.deposit_cashu);
    assert!(!snapshot.projection.capabilities.pay_bolt11);
    assert!(snapshot.projection.balances.is_empty());
    assert_eq!(snapshot.projection.accepted_mint_count, 0);
}

#[test]
fn snapshot_never_leaks_a_quote_id_proof_or_secret() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.created = true;
        state.mints = vec!["https://testnut.cashu.space".to_string()];
        state.cashu_pubkey_hex = Some("02".to_string() + &"a".repeat(64));
        state.pending_deposits.insert(
            "top-secret-quote-id".to_string(),
            state::PendingDeposit {
                operation_id: crate::journal::WalletOperationId::new("op-1"),
                mint: "https://testnut.cashu.space".to_string(),
                amount_sats: 21,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: None,
            },
        );
        state.ledger.apply(crate::journal::WalletFact::TokenAdded {
            token_event: crate::journal::WalletEventId::new("token-1"),
            mint: crate::journal::MintUrl::new("https://testnut.cashu.space"),
            unit: crate::journal::WalletUnit::new("sat"),
            proofs: vec![crate::journal::ProofAtom {
                proof: crate::journal::ProofRef::new("proof-secret-marker"),
                amount_msat: 21_000,
            }],
            via: crate::journal::Provenance::MintRollover,
        });
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let json = serde_json::to_string(&snapshot.projection).expect("projection serializes");
    for forbidden in [
        "top-secret-quote-id",
        "proof-secret-marker",
        "secret",
        "nsec",
    ] {
        assert!(
            !json.contains(forbidden),
            "projection JSON leaked forbidden marker {forbidden}: {json}"
        );
    }
    assert_eq!(snapshot.projection.readiness, WalletReadiness::Ready);
    assert_eq!(snapshot.projection.accepted_mint_count, 1);
    assert_eq!(snapshot.projection.balances[0].amount, 21);
}

const SNAPSHOT_ACCOUNT: &str = "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1";

/// #2949 — once a `RedeemNutzap` operation reaches `Settled`, it must show up
/// BOTH as an accepted `receive_rows` candidate and as a `recent_history` row
/// — before this fix `CashuWalletBackend::snapshot()` never called
/// `.with_recent_history(...)`/`.with_receive_rows(...)` at all, so a
/// completed redemption was invisible anywhere in the projection. Drives
/// `redeem::finish_redeem` for real (the same sanctioned direct-drive seam
/// `redeem_tests::finish_redeem_publishes_token_then_history_and_settles`
/// uses) rather than poking journal state by hand, so this proves the real
/// sign/publish/fold path actually leaves a row `snapshot()` can read.
#[test]
fn snapshot_surfaces_an_accepted_receive_row_and_history_row_after_settle() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let our_pubkey = "02".to_string() + &"cc".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(our_pubkey);
    let operation_id = crate::journal::WalletOperationId::new("cid-snapshot-redeem");
    let nutzap_event_id = "e".repeat(64);
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
            .transition(&operation_id, WalletOperationState::MintPending)
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
    let fresh_proofs = vec![synthetic_proof(21, "02fresh-snapshot")];

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    redeem::finish_redeem(redeem::FinishRedeemArgs {
        worker_tx: nmp_core::CommandSender::new(worker_tx),
        state: std::sync::Arc::clone(&backend.state),
        operation_id: operation_id.clone(),
        account_pubkey: SNAPSHOT_ACCOUNT.to_string(),
        nutzap,
        nutzap_wallet_event: crate::journal::WalletEventId::new(nutzap_event_id.clone()),
        fresh_proofs,
        relays: vec!["wss://my-relay.example".to_string()],
        created_at: 1_700_000_000,
        correlation_id: Some("cid-snapshot-redeem".to_string()),
    });

    // Drive the encrypt->sign->publish chain to completion, mirroring
    // `redeem_tests::finish_redeem_publishes_token_then_history_and_settles`.
    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::Nip44EncryptForAccount {
            continuation,
            ..
        }) => continuation,
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-token-ciphertext".to_string()));
    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            continuation, ..
        }) => continuation,
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "f".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: SNAPSHOT_ACCOUNT.to_string(),
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
    history_encrypt_continuation.call(Ok("fake-history-ciphertext".to_string()));
    let history_sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(nmp_core::actor::SignCommand::EventForAccount {
            continuation, ..
        }) => continuation,
        other => panic!("expected EventForAccount for history, got {other:?}"),
    };
    history_sign_continuation.call(Ok(nmp_signer_iface::SignedEvent {
        id: "1".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: SNAPSHOT_ACCOUNT.to_string(),
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

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let receive_row = snapshot
        .projection
        .receive_rows
        .iter()
        .find(|row| row.event_id == nutzap_event_id)
        .expect("a settled redeem must surface a receive row");
    assert!(receive_row.accepted);
    assert_eq!(receive_row.mint, MINT);
    assert_eq!(receive_row.amount, 21);

    let history_row = snapshot
        .projection
        .recent_history
        .iter()
        .find(|row| row.operation_id == operation_id.as_str())
        .expect("a settled redeem must surface a history row");
    assert_eq!(
        history_row.kind,
        crate::projection::WalletHistoryKind::RedeemNutzap
    );
    assert_eq!(history_row.amount, 21);
    assert_eq!(history_row.state, "Settled");
}

/// #2949 — the design doc's "Observer counting: unverifiable nutzaps may be
/// shown as rejected... not counted as value" requires a rejected candidate
/// to still surface, never disappear. Drives the exact observer path
/// (`on_wallet_event`) `runtime.rs` calls in production for an inbound
/// kind:9321, mirroring
/// `redeem_tests::on_wallet_event_dispatches_redeem_and_a_deliberately_unverifiable_nutzap_lands_failed`,
/// then asserts what THAT test doesn't: `snapshot()`'s projection actually
/// carries the rejected row, with no secret/proof material leaking into it.
///
/// Also #2966: a nutzap feed's "from <pubkey> at <time>" needs a sender and
/// a timestamp on both the receive row and the history row — this asserts
/// both are present even for a REJECTED redeem (recorded the moment the
/// event resolves in `RedeemNutzapCommand::run`, before the untrusted-mint
/// check below fails it), never only on a settled one.
#[test]
fn snapshot_surfaces_a_rejected_receive_row_for_an_unverifiable_nutzap() {
    let backend = backend_with_mint();
    let sender = nostr::Keys::generate();
    let proofs = vec![NutZapProof {
        amount: 21,
        id: "keyset-1".to_string(),
        secret: p2pk_secret("unused-lock-target"),
        c: "02aa".to_string(),
        dleq: None,
    }];
    let event = nutzap_kernel_event(
        &sender,
        proofs,
        "https://untrusted-mint.example",
        SNAPSHOT_ACCOUNT,
        None,
        None,
        1_699_999_500,
    );
    let event_id = event.id.clone();

    let commands = backend.on_wallet_event(
        WalletBackendContext {
            now_secs: 1_700_000_000,
            selected_backend: None,
            account_pubkey: Some(SNAPSHOT_ACCOUNT),
        },
        &event,
    );
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

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let receive_row = snapshot
        .projection
        .receive_rows
        .iter()
        .find(|row| row.event_id == event_id)
        .expect("a rejected nutzap must still surface as a receive row, never absent");
    assert!(!receive_row.accepted);
    assert_eq!(receive_row.mint, "https://untrusted-mint.example");
    assert_eq!(receive_row.amount, 21);
    assert_eq!(
        receive_row.sender.as_deref(),
        Some(sender.public_key().to_hex().as_str()),
        "a nutzap feed needs to show who a rejected nutzap was from too"
    );
    assert_eq!(receive_row.timestamp, Some(1_700_000_000));

    let history_row = snapshot
        .projection
        .recent_history
        .iter()
        .find(|row| row.kind == crate::projection::WalletHistoryKind::RedeemNutzap)
        .expect("a rejected redeem must surface a history row");
    assert_eq!(history_row.state, "Failed");
    assert_eq!(
        history_row.sender.as_deref(),
        Some(sender.public_key().to_hex().as_str())
    );
    assert_eq!(history_row.timestamp, Some(1_700_000_000));

    let json = serde_json::to_string(&snapshot.projection).expect("projection serializes");
    for forbidden in ["secret", "nsec", "proof", "quote_id"] {
        assert!(
            !json.contains(forbidden),
            "projection JSON leaked forbidden marker {forbidden}: {json}"
        );
    }
}
