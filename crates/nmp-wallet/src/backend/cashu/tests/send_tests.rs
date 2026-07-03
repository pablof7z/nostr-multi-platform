//! `SendNutzap` — W8 (#2917): fail-closed recipient/mint/balance gates
//! (`SendNutzapCommand::run`) and the post-swap fold + kind:9321 publish
//! (`send::finish_send`, driven directly with synthetic proofs — the mint
//! HTTP/DHKE round-trip itself is `nmp-nip60`'s own tested surface).

use super::*;
use nmp_core::actor::PublishCommand;
use nmp_core::publish::PublishTarget;
use nmp_nip60::kinds::KIND_NIP61_NUTZAP;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

fn run_send(
    backend: &CashuWalletBackend,
    cached: &FixedCachedEvents,
    correlation_id: &str,
    amount_sats: u64,
) -> RecordingErrorSurface {
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::SendNutzap {
            recipient_pubkey: RECIPIENT.to_string(),
            amount_sats,
            target_event_id: None,
        },
        Some(correlation_id.to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
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

#[test]
fn no_recipient_info_fails_closed() {
    let backend = backend_with_mint();
    let errors = run_send(&backend, &FixedCachedEvents::default(), "cid-no-info", 21);
    expect_error_code(&errors, ui_codes::NO_RECIPIENT_NUTZAP_INFO);
}

#[test]
fn recipient_info_with_no_relays_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: Vec::new(),
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);
    let errors = run_send(&backend, &cached, "cid-no-relays", 21);
    expect_error_code(&errors, ui_codes::NO_RECIPIENT_RELAYS);
}

#[test]
fn recipient_info_with_no_p2pk_pubkey_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: None,
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);
    let errors = run_send(&backend, &cached, "cid-no-p2pk", 21);
    expect_error_code(&errors, ui_codes::NO_RECIPIENT_P2PK);
}

#[test]
fn no_overlapping_mint_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec!["https://a-different-mint.example".to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);
    let errors = run_send(&backend, &cached, "cid-no-mint", 21);
    expect_error_code(&errors, ui_codes::NO_TRUSTED_MINT);
}

#[test]
fn insufficient_balance_fails_closed() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);
    // No proofs at all in `backend`'s state.
    let errors = run_send(&backend, &cached, "cid-insufficient", 21);
    expect_error_code(&errors, ui_codes::INSUFFICIENT_BALANCE);
}

/// The core saga invariant, mirroring `deposit_tests`'s equivalent: consumed
/// inputs are journaled and the operation reaches `MintPending` synchronously
/// inside `start_intent`/`run` up to the point the mint swap would fire —
/// this test proves the ordering without any network access by stopping the
/// backend's own proof selection from ever finding a mint reachable (the
/// happy path below drives the mint-HTTP-free `finish_send` directly).
#[test]
fn insufficient_balance_never_reaches_mint_pending() {
    let backend = backend_with_mint();
    let info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://recipient-relay.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"44".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(RECIPIENT, &info, 1_699_999_000)]);
    let _rx = run_send(&backend, &cached, "cid-op-state", 21);
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-op-state"))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::Failed,
        "insufficient balance must fail closed before any mint HTTP call"
    );
}

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
