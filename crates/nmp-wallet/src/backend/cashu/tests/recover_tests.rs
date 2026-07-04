//! `RecoverCashuWallet` (#2965) — the explicit `nmp.wallet.cashu.recover`
//! action path: fail-closed when no cached kind:17375 exists for this
//! account, idempotent success when a wallet is already loaded, and the
//! happy-path decrypt->`ingest_wallet_config`->`RecordActionSuccess` chain.

use super::*;
use nmp_core::actor::{ActionLedgerCommand, SignCommand};
use nmp_core::substrate::KernelEvent;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

fn wallet_event_fixture(account: &str, content: &str) -> KernelEvent {
    KernelEvent {
        id: "e".repeat(64),
        author: account.to_string(),
        kind: nmp_nip60::kinds::KIND_NIP60_WALLET,
        created_at: 1_700_000_000,
        tags: Vec::new(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

/// No cached kind:17375 for this account — `ctx.latest_author_kind` returns
/// `None`, so `run()` fails closed with the documented code rather than
/// dispatching a decrypt that could never resolve.
#[test]
fn no_existing_wallet_fails_closed_with_the_documented_code() {
    let backend = CashuWalletBackend::new();
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::RecoverCashuWallet,
        Some("cid-recover".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let cached_events = FixedCachedEvents::default();
    let errors = RecordingErrorSurface::default();
    let (worker_tx, _worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let mut c = ctx_with_cached_events_and_errors(
        &send,
        nmp_core::CommandSender::new(worker_tx),
        &clock,
        &recipients,
        &cached_events,
        &errors,
    );
    cmd.run(&mut c).expect("run returns Ok");

    assert_eq!(
        errors.last_token_code.lock().unwrap().as_deref(),
        Some(ui_codes::NO_EXISTING_WALLET)
    );
    assert_eq!(
        errors.failures.lock().unwrap().as_slice(),
        &[(
            "cid-recover".to_string(),
            "no existing kind:17375 wallet found on relays for this account".to_string()
        )]
    );
}

/// A wallet already loaded (by a prior create, a prior recover, or the
/// passive replay path winning the race) makes `RecoverCashuWallet` an
/// idempotent success — never re-decrypts, never re-dispatches.
#[test]
fn already_created_recovers_idempotently() {
    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).created = true;
    let account = "bb".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::RecoverCashuWallet,
        Some("cid-idem".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

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

    // `ctx.record_action_success` reports synchronously through `send`
    // (the sink), never the worker channel — distinct from the async
    // decrypt-chain's `build_record_action_success`, which lands on the
    // worker channel instead (see the happy-path test below).
    assert!(
        worker_rx.try_recv().is_err(),
        "an idempotent no-op must never dispatch a decrypt"
    );
    let sends = sink.sends.lock().unwrap();
    assert!(matches!(
        sends.as_slice(),
        [ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess { correlation_id, .. })]
            if correlation_id == "cid-idem"
    ));
}

/// The full happy path: a cached kind:17375 is found, decrypted, and folded
/// into state, and the action reports success on the worker channel.
#[test]
fn happy_path_loads_the_existing_wallet_and_reports_success() {
    let backend = CashuWalletBackend::new();
    let account = "cc".repeat(32);
    let cached_events = FixedCachedEvents(vec![wallet_event_fixture(&account, "fake-ciphertext")]);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::RecoverCashuWallet,
        Some("cid-happy".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached_events,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    let decrypt_continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44DecryptForAccount {
            peer_pubkey,
            ciphertext,
            signer_pubkey,
            continuation,
        }) => {
            assert_eq!(peer_pubkey, account, "self-decrypt targets the account's own pubkey");
            assert_eq!(ciphertext, "fake-ciphertext");
            assert_eq!(signer_pubkey.as_deref(), Some(account.as_str()));
            continuation
        }
        other => panic!("expected Nip44DecryptForAccount, got {other:?}"),
    };

    let sk = nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let plaintext = serde_json::to_string(&vec![
        vec!["privkey".to_string(), hex::encode(sk.secret_bytes())],
        vec!["mint".to_string(), MINT.to_string()],
    ])
    .unwrap();
    decrypt_continuation.call(Ok(plaintext));

    match recv_command(&worker_rx) {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess { correlation_id, .. }) => {
            assert_eq!(correlation_id, "cid-happy");
        }
        other => panic!("expected RecordActionSuccess, got {other:?}"),
    }

    let state = lock_state(&backend.state);
    assert!(state.created);
    assert_eq!(state.mints, vec![MINT.to_string()]);
}

/// A signer that cannot NIP-44 decrypt fails closed — no partial state, and a
/// definitive, correlation-id-backed failure lands on the worker channel
/// (distinct from the passive `on_self_authored_wallet_event` path, which
/// stays silent — see `ingest.rs`'s module docs).
#[test]
fn signer_cannot_nip44_fails_closed() {
    let backend = CashuWalletBackend::new();
    let account = "dd".repeat(32);
    let cached_events = FixedCachedEvents(vec![wallet_event_fixture(&account, "fake-ciphertext")]);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::RecoverCashuWallet,
        Some("cid-fail".to_string()),
    );
    let ActorCommand::Protocol(cmd) = commands.into_iter().next().unwrap() else {
        panic!("expected a Protocol command");
    };

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(0);
    let recipients = FixedRecipientLookup(Vec::new());
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ctx_with_cached_events(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
            &cached_events,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    let continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44DecryptForAccount { continuation, .. }) => {
            continuation
        }
        other => panic!("expected Nip44DecryptForAccount, got {other:?}"),
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

    assert!(
        !lock_state(&backend.state).created,
        "a failed decrypt must never mark the wallet created"
    );
}
