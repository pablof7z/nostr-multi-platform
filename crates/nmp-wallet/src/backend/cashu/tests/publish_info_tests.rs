//! `PublishNutzapInfo` — W13 (#2917): fail-closed gates and the plain
//! sign->publish chain `PublishNutzapInfoCommand::run` drives.

use super::*;
use nmp_core::actor::{PublishCommand, SignCommand};
use nmp_core::publish::PublishTarget;
use nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO;
use nmp_signer_iface::SignedEvent;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

#[test]
fn no_active_account_fails_closed() {
    let backend = CashuWalletBackend::new();
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::PublishNutzapInfo,
        Some("cid-1".to_string()),
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::NO_ACCOUNT);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn no_cashu_wallet_fails_closed_inside_run() {
    let backend = CashuWalletBackend::new();
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::PublishNutzapInfo,
        Some("cid-no-wallet".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://relay.example".to_string()]);
    let cached = FixedCachedEvents::default();
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
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
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        Some(ui_codes::NO_CASHU_WALLET)
    );
}

/// A wallet with mints/pubkey but no cached self kind:10019 and no NIP-65
/// fallback (`FixedRecipientLookup` returning empty) must fail closed rather
/// than publish with an empty relay set.
#[test]
fn no_relays_resolved_fails_closed() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"11".repeat(32));
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::PublishNutzapInfo,
        Some("cid-no-relays".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let cached = FixedCachedEvents::default();
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
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
    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        Some(ui_codes::NO_NUTZAP_RELAYS)
    );
}

/// Happy path with no cached self kind:10019: falls back to
/// `recipient_publish_relays`, builds kind:10019 tags from mints/pubkey/
/// relays, and drives sign->publish (no NIP-44 step — kind:10019 is public).
#[test]
fn happy_path_falls_back_to_nip65_relays_and_publishes_kind_10019() {
    let backend = backend_with_mint();
    let pubkey_hex = "02".to_string() + &"22".repeat(32);
    lock_state(&backend.state).cashu_pubkey_hex = Some(pubkey_hex.clone());
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::PublishNutzapInfo,
        Some("cid-happy".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://fallback.example".to_string()]);
    let cached = FixedCachedEvents::default();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    let sign_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned,
            signer_pubkey,
            continuation,
        }) => {
            assert_eq!(unsigned.kind, KIND_NIP61_NUTZAP_INFO);
            assert_eq!(signer_pubkey.as_deref(), Some(ACCOUNT));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["relay".to_string(), "wss://fallback.example".to_string()]));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["mint".to_string(), MINT.to_string()]));
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["pubkey".to_string(), pubkey_hex.clone()]));
            continuation
        }
        other => panic!("expected EventForAccount (no nip44 step), got {other:?}"),
    };
    let signed = SignedEvent {
        id: "e".repeat(64),
        sig: "s".repeat(128),
        unsigned: nmp_signer_iface::UnsignedEvent {
            pubkey: ACCOUNT.to_string(),
            kind: KIND_NIP61_NUTZAP_INFO,
            tags: Vec::new(),
            content: String::new(),
            created_at: 1_700_000_000,
        },
    };
    sign_continuation.call(Ok(signed));

    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            target,
            correlation_id,
        }) => {
            assert_eq!(raw.kind, KIND_NIP61_NUTZAP_INFO);
            assert_eq!(correlation_id.as_deref(), Some("cid-happy"));
            match target {
                PublishTarget::Explicit { relays, .. } => {
                    assert_eq!(relays, vec!["wss://fallback.example".to_string()]);
                }
                other => panic!("expected explicit publish target, got {other:?}"),
            }
        }
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-happy"))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::PublishPending
    );
}

/// When this account already has a cached kind:10019 with non-empty relays,
/// those relays win over `recipient_publish_relays`'s fallback.
#[test]
fn prefers_the_self_cached_kind_10019_relays_over_nip65_fallback() {
    let backend = backend_with_mint();
    lock_state(&backend.state).cashu_pubkey_hex = Some("02".to_string() + &"33".repeat(32));
    let commands = backend.start_intent(
        ctx(Some(ACCOUNT)),
        WalletIntent::PublishNutzapInfo,
        Some("cid-cached".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(vec!["wss://should-not-be-used.example".to_string()]);
    let cached_info = nmp_nip60::nutzap::NutZapInfo {
        relays: vec!["wss://cached.example".to_string()],
        mints: vec![MINT.to_string()],
        cashu_pubkey: Some("02".to_string() + &"33".repeat(32)),
    };
    let cached = FixedCachedEvents(vec![nutzap_info_kernel_event(
        ACCOUNT,
        &cached_info,
        1_699_999_000,
    )]);
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::EventForAccount { unsigned, .. }) => {
            assert!(unsigned
                .tags
                .iter()
                .any(|t| t == &vec!["relay".to_string(), "wss://cached.example".to_string()]));
            assert!(!unsigned.tags.iter().any(|t| t
                == &vec![
                    "relay".to_string(),
                    "wss://should-not-be-used.example".to_string()
                ]));
        }
        other => panic!("expected EventForAccount, got {other:?}"),
    }
}
