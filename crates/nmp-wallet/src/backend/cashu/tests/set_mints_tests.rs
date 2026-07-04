//! `SetCashuMints` (#2997) — fail-closed gates in `start_set_mints`, and the
//! money-critical invariant `SetCashuMintsCommand::run` must uphold: the
//! kind:17375 plaintext it builds carries the EXISTING Cashu P2PK privkey
//! forward unchanged (never `WalletConfig::generate`s a fresh one), with only
//! the `mint` entries replaced.

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

/// A `CashuWalletBackend` that already has a created wallet: a real (random,
/// test-only) Cashu P2PK privkey/pubkey and `mints` already set — the
/// precondition `SetCashuMints` requires. Returns the backend plus the
/// privkey's hex (`display_secret()`) so tests can assert it round-trips
/// through the command unchanged.
fn backend_with_existing_wallet(existing_mints: Vec<&str>) -> (CashuWalletBackend, String) {
    let backend = CashuWalletBackend::new();
    let sk = nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let sk_hex = sk.display_secret().to_string();
    {
        let mut state = state::lock_state(&backend.state);
        state.created = true;
        state.mints = existing_mints.into_iter().map(str::to_string).collect();
        state.cashu_pubkey_hex = Some("02".to_string() + &"aa".repeat(32));
        state.cashu_privkey = Some(state::CashuP2pkSecret(sk));
    }
    (backend, sk_hex)
}

#[test]
fn no_active_account_fails_closed_without_dispatching() {
    let (backend, _sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::SetCashuMints {
            mints: vec!["https://new-mint.example".to_string()],
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
}

#[test]
fn empty_mint_list_fails_closed() {
    let (backend, _sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::SetCashuMints { mints: Vec::new() },
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
fn malformed_mint_url_fails_closed() {
    let (backend, _sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::SetCashuMints {
            mints: vec!["not-a-url".to_string()],
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

/// The core fail-closed precondition: a wallet that was never created/
/// recovered has no existing privkey to carry forward, and this action must
/// never mint a fresh one — that would silently do what `cashu.create` is
/// for, defeating the whole point of a key-PRESERVING edit.
#[test]
fn no_existing_wallet_fails_closed_with_no_cashu_wallet() {
    let backend = CashuWalletBackend::new();
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::SetCashuMints {
            mints: vec!["https://new-mint.example".to_string()],
        },
        None,
    );
    assert!(
        commands
            .iter()
            .all(|c| !matches!(c, ActorCommand::Protocol(_))),
        "no existing wallet -> never reaches the Protocol dispatch: {commands:?}"
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::NO_CASHU_WALLET);
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn valid_set_mints_journals_prepared_before_dispatch() {
    let (backend, _sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::SetCashuMints {
            mints: vec!["https://new-mint.example".to_string()],
        },
        Some("cid-set-mints".to_string()),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::Protocol(_)));

    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-set-mints"))
        .expect("operation recorded before dispatch");
    assert_eq!(op.kind, WalletOperationKind::SetCashuMints);
}

/// Money-critical: the kind:17375 plaintext `SetCashuMintsCommand::run`
/// builds must carry the wallet's PRE-EXISTING Cashu P2PK privkey forward
/// byte-identical — never a freshly generated one — with the `mint` entries
/// replaced by the caller-supplied list. `on_signed` must update ONLY
/// `state.mints`; `cashu_pubkey_hex`/`cashu_privkey` must be untouched.
#[test]
fn happy_path_carries_forward_the_existing_privkey_and_replaces_mints() {
    let (backend, sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let original_pubkey_hex = state::lock_state(&backend.state)
        .cashu_pubkey_hex
        .clone()
        .expect("fixture sets a pubkey");
    let account = "bb".repeat(32);
    let new_mints = vec![
        "https://new-mint-a.example".to_string(),
        "https://new-mint-b.example".to_string(),
    ];
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::SetCashuMints {
            mints: new_mints.clone(),
        },
        Some("cid-happy-set-mints".to_string()),
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
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    // Step 1: Nip44EncryptForAccount — the plaintext is where the
    // money-critical assertion lives: `privkey` must be the PRE-EXISTING
    // hex, and `mint` entries must be exactly `new_mints`, nothing stale.
    let (plaintext, encrypt_continuation) = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
            peer_pubkey,
            signer_pubkey,
            plaintext,
            continuation,
            ..
        }) => {
            assert_eq!(peer_pubkey, account);
            assert_eq!(signer_pubkey.as_deref(), Some(account.as_str()));
            (plaintext, continuation)
        }
        other => panic!("expected Nip44EncryptForAccount, got {other:?}"),
    };
    let pairs: Vec<Vec<String>> =
        serde_json::from_str(&plaintext).expect("plaintext must be a JSON array of pairs");
    assert_eq!(
        pairs[0],
        vec!["privkey".to_string(), sk_hex.clone()],
        "the FIRST pair must be the wallet's pre-existing privkey, never a fresh one"
    );
    let encoded_mints: Vec<&String> = pairs[1..]
        .iter()
        .map(|pair| {
            assert_eq!(pair[0], "mint");
            &pair[1]
        })
        .collect();
    assert_eq!(
        encoded_mints,
        new_mints.iter().collect::<Vec<_>>(),
        "the mint list must be exactly the caller-supplied replacement, in order"
    );
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

    // Step 3: Publish(SignedEvent).
    match recv_command(&worker_rx) {
        ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            target,
            correlation_id,
        }) => {
            assert_eq!(raw.kind, nmp_nip60::KIND_NIP60_WALLET);
            assert_eq!(correlation_id.as_deref(), Some("cid-happy-set-mints"));
            match target {
                PublishTarget::Explicit {
                    relays,
                    route_class,
                } => {
                    assert_eq!(relays, vec!["wss://relay.example".to_string()]);
                    assert_eq!(route_class, PublishRouteClass::ImportedOrPresigned);
                }
                other => panic!("expected an explicit publish target, got {other:?}"),
            }
        }
        other => panic!("expected Publish(SignedEvent), got {other:?}"),
    }

    // `on_signed` ran before the publish was enqueued: `mints` replaced,
    // `cashu_pubkey_hex` UNCHANGED (never rotated).
    let state = state::lock_state(&backend.state);
    assert_eq!(state.mints, new_mints);
    assert_eq!(state.cashu_pubkey_hex, Some(original_pubkey_hex));
    assert_eq!(
        state
            .cashu_privkey
            .as_ref()
            .unwrap()
            .0
            .display_secret()
            .to_string(),
        sk_hex,
        "the Cashu P2PK privkey must be byte-identical to the pre-existing one"
    );
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new(
            "cid-happy-set-mints",
        ))
        .unwrap();
    assert_eq!(
        op.state,
        crate::journal::WalletOperationState::PublishPending
    );
}

/// Signer-can't-NIP-44 fails closed exactly as `cashu.create`'s equivalent
/// gate does: no publish, `mints` stays the OLD list (never partially
/// applied), and a `ShowErrorToken`/`RecordActionFailure` land on the worker
/// channel.
#[test]
fn signer_cannot_nip44_fails_closed_without_touching_mints() {
    let (backend, _sk_hex) = backend_with_existing_wallet(vec!["https://old-mint.example"]);
    let account = "cc".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::SetCashuMints {
            mints: vec!["https://new-mint.example".to_string()],
        },
        Some("cid-fail-set-mints".to_string()),
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
        let mut c = ctx_with_sender(
            &send,
            nmp_core::CommandSender::new(worker_tx),
            &clock,
            &recipients,
        );
        cmd.run(&mut c).expect("run returns Ok");
    }

    let continuation = match recv_command(&worker_rx) {
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { continuation, .. }) => {
            continuation
        }
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
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
            correlation_id, ..
        }) => {
            assert_eq!(correlation_id, "cid-fail-set-mints");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }

    let state = state::lock_state(&backend.state);
    assert_eq!(
        state.mints,
        vec!["https://old-mint.example".to_string()],
        "a fail-closed encrypt must never touch the accepted-mint list"
    );
}
