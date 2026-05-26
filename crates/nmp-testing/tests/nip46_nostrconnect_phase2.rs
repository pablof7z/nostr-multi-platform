//! T-nostrconnect-phase2 — NIP-46 nostrconnect:// client-initiated handshake.
//!
//! RED phase: these tests are written against the public API that Phase 2 must
//! expose. They will FAIL until the implementation lands (all annotated with
//! `// RED` to make that clear).
//!
//! ## What is tested
//!
//! 1. **Happy path**: a mock signer app scans the QR, connects to the relay,
//!    sends a `connect` RPC with the correct secret. The broker:
//!    a. Learns the signer's pubkey from `event.pubkey`.
//!    b. Validates the secret.
//!    c. Replies `ack`.
//!    d. Sends `get_public_key`.
//!    e. Receives the user pubkey.
//!    f. Ships `AddRemoteSigner` to the actor.
//!    The actor command channel must receive `AddRemoteSigner` with the user
//!    pubkey within 10 s.
//!
//! 2. **Wrong secret**: the mock sends a `connect` with a wrong secret. The
//!    broker must NOT emit `AddRemoteSigner`. Instead it must emit a
//!    `BunkerHandshakeProgress { stage: "failed", ... }` within 10 s and no
//!    session is established.
//!
//! ## Architecture
//!
//! The broker calls `start_nostrconnect_handshake(relay_url) -> String` which:
//! - Generates ephemeral keypair + secret.
//! - Returns the `nostrconnect://` URI immediately.
//! - Spawns a worker that subscribes to the relay and awaits the signer's
//!   `connect` event (signer-initiated direction, opposite of `bunker://`).
//!
//! The mock (`MockNostrConnectSigner`) acts as the signer-app:
//! - Connects to the relay ws://127.0.0.1:<port>.
//! - Reads the ephemeral client pubkey + secret from the URI.
//! - Sends a kind:24133 `connect` RPC encrypted to the client pubkey.
//! - Waits for the client's `get_public_key` RPC.
//! - Replies with `user_keys.public_key().to_hex()`.

mod common;

use std::sync::mpsc;
use std::time::Duration;

use nmp_core::ActorCommand;
use nostr::Keys;

use crate::common::broker_adapter::broker_for_actor;
use crate::common::mock_nostrconnect_signer::MockNostrConnectSigner;

/// Happy-path nostrconnect:// handshake: signer connects with correct secret,
/// broker emits AddRemoteSigner carrying the user pubkey.
#[test]
fn nostrconnect_happy_path_emits_add_remote_signer() {
    // ── Setup ─────────────────────────────────────────────────────────────
    // The mock will play the role of the signer app.
    // `user_keys` is the keypair the mock will report as the user identity.
    let user_keys = Keys::generate();
    let mock = MockNostrConnectSigner::spawn(user_keys.clone())
        .expect("mock nostrconnect signer must spawn on 127.0.0.1");

    let (actor_tx, actor_rx) = mpsc::channel::<ActorCommand>();
    let broker = broker_for_actor(actor_tx);

    // ── Generate URI + start listening ───────────────────────────────────
    // `start_nostrconnect_handshake` returns the URI synchronously and spawns
    // the relay-subscription worker. The mock will parse the URI and act as
    // the signer app.
    let relay_url = mock.ws_url();
    let uri = broker.start_nostrconnect_handshake(relay_url);

    assert!(
        uri.starts_with("nostrconnect://"),
        "URI must start with nostrconnect://, got: {uri}"
    );

    // Hand the URI to the mock so it can extract the ephemeral pubkey + secret
    // and drive the connect RPC.
    mock.connect_with_correct_secret(&uri);

    // ── Wait for AddRemoteSigner ──────────────────────────────────────────
    let handle = wait_for_add_remote_signer(&actor_rx, Duration::from_secs(10))
        .expect("AddRemoteSigner must arrive on the actor channel within 10 s");

    assert_eq!(
        handle.pubkey_hex(),
        user_keys.public_key().to_hex(),
        "signer must report the user pubkey returned by get_public_key",
    );
    assert_eq!(handle.signer_kind(), "nip46");

    broker.cancel();
}

/// Negative: signer sends connect with wrong secret. Broker must emit
/// BunkerHandshakeProgress { stage: "failed" } and NOT ship AddRemoteSigner.
#[test]
fn nostrconnect_wrong_secret_fails_handshake() {
    let user_keys = Keys::generate();
    let mock = MockNostrConnectSigner::spawn(user_keys.clone())
        .expect("mock nostrconnect signer must spawn");

    let (actor_tx, actor_rx) = mpsc::channel::<ActorCommand>();
    let broker = broker_for_actor(actor_tx);

    let relay_url = mock.ws_url();
    let uri = broker.start_nostrconnect_handshake(relay_url);

    // Send a connect with a clearly wrong secret (all zeros).
    mock.connect_with_wrong_secret(&uri, "0000000000000000");

    // Expect a "failed" progress event within 10 s and no AddRemoteSigner.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut saw_failed = false;
    let mut saw_add_remote = false;
    loop {
        let remaining = match deadline.checked_duration_since(std::time::Instant::now()) {
            Some(r) => r,
            None => break,
        };
        match actor_rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(ActorCommand::BunkerHandshakeProgress { stage, .. }) if stage == "failed" => {
                saw_failed = true;
                break;
            }
            Ok(ActorCommand::AddRemoteSigner { .. }) => {
                saw_add_remote = true;
                break;
            }
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    assert!(saw_failed, "broker must emit stage=failed for wrong secret");
    assert!(
        !saw_add_remote,
        "broker must NOT emit AddRemoteSigner for wrong secret"
    );

    broker.cancel();
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn wait_for_add_remote_signer(
    actor_rx: &mpsc::Receiver<ActorCommand>,
    timeout: Duration,
) -> Option<Box<dyn nmp_core::RemoteSignerHandle>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        match actor_rx.recv_timeout(remaining) {
            Ok(ActorCommand::AddRemoteSigner { handle }) => return Some(handle),
            Ok(ActorCommand::BunkerHandshakeProgress { stage, message }) => {
                if stage == "failed" {
                    panic!("nostrconnect handshake failed: {stage}: {message:?}");
                }
                continue;
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}
