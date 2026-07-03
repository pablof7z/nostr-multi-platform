//! `CreateCashuWallet` — fail-closed gates in `start_intent`, and the
//! encrypt->sign->publish chain `CreateCashuWalletCommand::run` drives.

use super::*;
use nmp_core::actor::{ActionLedgerCommand, PublishCommand, SignCommand};
use nmp_core::publish::{PublishRouteClass, PublishTarget};
use nmp_signer_iface::SignedEvent;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

#[test]
fn no_active_account_fails_closed_without_dispatching() {
    let backend = CashuWalletBackend::new();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::CreateCashuWallet {
            mint: "https://testnut.cashu.space".to_string(),
        },
        Some("cid-1".to_string()),
    );
    assert!(
        commands
            .iter()
            .all(|c| !matches!(c, ActorCommand::Protocol(_))),
        "no account -> never reaches the Protocol dispatch: {commands:?}"
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::NO_ACCOUNT);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
    assert!(matches!(
        &commands[1],
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. })
            if correlation_id == "cid-1"
    ));
}

#[test]
fn unsupported_mint_url_fails_closed() {
    let backend = CashuWalletBackend::new();
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::CreateCashuWallet {
            mint: "not-a-url".to_string(),
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::UNSUPPORTED_MINT);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn valid_create_journals_prepared_before_dispatch() {
    let backend = CashuWalletBackend::new();
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CreateCashuWallet {
            mint: "https://testnut.cashu.space".to_string(),
        },
        Some("cid-create".to_string()),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::Protocol(_)));

    // Synchronous, pre-dispatch journal write — no HTTP/port round-trip has
    // happened yet, but the operation already exists as `Prepared`.
    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-create"))
        .expect("operation recorded before dispatch");
    assert_eq!(op.kind, WalletOperationKind::CreateCashuWallet);
}

/// Drive `CreateCashuWalletCommand::run` through the full chain and assert
/// the terminal `Publish(SignedEvent)` + the state mutation `on_signed` makes.
#[test]
fn happy_path_signs_and_publishes_kind_17375() {
    let backend = CashuWalletBackend::new();
    let account = "bb".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CreateCashuWallet {
            mint: "https://testnut.cashu.space".to_string(),
        },
        Some("cid-happy".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_sender(&send, nmp_core::CommandSender::new(worker_tx), &clock, &recipients);
        cmd.run(&mut c).expect("run returns Ok");
    }

    // Step 1: Nip44EncryptForAccount lands on the worker channel.
    let encrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
            peer_pubkey,
            signer_pubkey,
            continuation,
            ..
        }) => {
            assert_eq!(peer_pubkey, account, "self-encrypt targets the account's own pubkey");
            assert_eq!(signer_pubkey.as_deref(), Some(account.as_str()));
            continuation
        }
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    encrypt_continuation.call(Ok("fake-ciphertext".to_string()));

    // Step 2: SignEventForAccount.
    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned,
            signer_pubkey,
            continuation,
        }) => {
            assert_eq!(unsigned.kind, nmp_nip60::KIND_NIP60_WALLET);
            assert_eq!(unsigned.content, "fake-ciphertext");
            assert_eq!(signer_pubkey.as_deref(), Some(account.as_str()));
            continuation
        }
        other => panic!("expected EventForAccount, got {other:?}"),
    };
    let signed = SignedEvent {
        id: "e".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: account.clone(),
            kind: nmp_nip60::KIND_NIP60_WALLET,
            tags: Vec::new(),
            content: "fake-ciphertext".to_string(),
            created_at: 1_700_000_000,
        },
    };
    sign_continuation.call(Ok(signed));

    // Step 3: Publish(SignedEvent) with an explicit, pre-signed route.
    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent { raw, target, correlation_id }) => {
            assert_eq!(raw.kind, nmp_nip60::KIND_NIP60_WALLET);
            assert_eq!(correlation_id.as_deref(), Some("cid-happy"));
            match target {
                PublishTarget::Explicit { relays, route_class } => {
                    assert_eq!(relays, vec!["wss://relay.example".to_string()]);
                    assert_eq!(route_class, PublishRouteClass::ImportedOrPresigned);
                }
                other => panic!("expected an explicit publish target, got {other:?}"),
            }
        }
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    // `on_signed` ran before the publish was enqueued.
    let state = state::lock_state(&backend.state);
    assert!(state.created);
    assert!(state.cashu_pubkey_hex.is_some());
    assert_eq!(state.mints, vec!["https://testnut.cashu.space".to_string()]);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-happy"))
        .unwrap();
    assert_eq!(op.state, crate::journal::WalletOperationState::PublishPending);
}

/// Signer-can't-NIP-44 fails closed: no publish, `created` stays false, and
/// the operation is NOT silently left at `Prepared` forever without a
/// surfaced failure — a `ShowErrorToken` + `RecordActionFailure` land on the
/// worker channel instead.
#[test]
fn signer_cannot_nip44_fails_closed() {
    let backend = CashuWalletBackend::new();
    let account = "cc".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CreateCashuWallet {
            mint: "https://testnut.cashu.space".to_string(),
        },
        Some("cid-fail".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_sender(&send, nmp_core::CommandSender::new(worker_tx), &clock, &recipients);
        cmd.run(&mut c).expect("run returns Ok");
    }

    let continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { continuation, .. }) => continuation,
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    continuation.call(Err("signer does not support nip44".to_string()));

    match recv_command(&worker_rx) {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::OPERATION_FAILED);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
    match recv_command(&worker_rx) {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure { correlation_id, .. }) => {
            assert_eq!(correlation_id, "cid-fail");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert!(!state.created, "a fail-closed encrypt must never mark the wallet created");
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-fail"))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::Prepared,
        "sign-can't-nip44 must not silently advance the journal"
    );
}
