use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use nmp_core::substrate::UnsignedEvent;
use nmp_core::RemoteSignerHandle;
use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

use super::*;

#[test]
fn new_and_cancel_are_noops_without_session() {
    let (tx, _rx) = mpsc::channel::<ActorCommand>();
    let broker = BunkerBroker::new(tx);
    broker.cancel();
    broker.cancel();
}

#[test]
fn start_handshake_with_invalid_uri_emits_failed_progress() {
    let (tx, rx) = mpsc::channel::<ActorCommand>();
    let broker = BunkerBroker::new(tx);
    broker.start_handshake("not-a-bunker-uri".to_string());

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("failed-progress event");
    assert!(
        matches!(
            event,
            ActorCommand::BunkerHandshakeProgress { ref stage, .. } if stage == "failed"
        ),
        "expected a failed-progress event for invalid URI"
    );
    broker.cancel();
}

#[test]
fn noop_relay_send_returns_disconnected_error() {
    let result = NoopRelay.send("[\"EVENT\",{}]".to_string());
    assert!(
        matches!(result, Err(crate::relay_client::RelayError::Disconnected)),
        "NoopRelay must reject sends, not drop them silently"
    );
}

#[test]
fn noop_relay_shutdown_is_a_noop() {
    NoopRelay.shutdown();
}

/// A transport whose `send_rpc` always succeeds — lets a `sign()` reach the
/// `Pending` state with a registered one-shot entry, without any relay I/O.
#[derive(Debug, Default)]
struct AcceptingTransport;

impl Nip46Transport for AcceptingTransport {
    fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
        Ok(())
    }
}

#[test]
fn arc_remote_signer_disconnect_drains_pending_sign() {
    // The actor holds the broker's `Nip46Signer` as a `Box<dyn
    // RemoteSignerHandle>` — concretely an `ArcRemoteSigner`. On
    // `RemoveAccount` the actor calls `handle.disconnect()` so blocked
    // `SignerOp::wait` callers fail fast instead of waiting out the 5s
    // remote-sign timeout.
    //
    // `ArcRemoteSigner` must therefore FORWARD `disconnect()` to the inner
    // signer. Without the forwarder the trait's default no-op runs and the
    // pending request hangs — this test pins the forwarding contract.
    let local = nmp_signers::SecretKey::from_hex(
        "0000000000000000000000000000000000000000000000000000000000000001",
    )
    .expect("valid secret hex");
    let remote_user = nmp_signers::SecretKey::from_hex(
        "0000000000000000000000000000000000000000000000000000000000000002",
    )
    .expect("valid secret hex");
    let remote_user_pubkey = nostr::Keys::new(remote_user).public_key();
    let uri = format!(
        "bunker://{}?relay=wss://relay.example.com",
        nostr::Keys::new(local.clone()).public_key().to_hex()
    );
    let handle = Nip46SignerHandle::from_bunker_uri_with_local_key(&uri, local)
        .expect("parse bunker uri");
    let signer = Arc::new(handle.complete(Arc::new(AcceptingTransport), remote_user_pubkey));

    // Start a sign() — the accepting transport leaves a Pending one-shot
    // registered in the signer's `pending` map.
    let wrapper = ArcRemoteSigner(Arc::clone(&signer));
    let unsigned = UnsignedEvent {
        pubkey: remote_user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "in flight".to_string(),
        created_at: 1_700_000_000,
    };
    let op = RemoteSignerHandle::sign(&wrapper, &unsigned);

    // Disconnect through the trait object the actor holds. This MUST drain
    // the pending request — surfacing an Err — not a timeout.
    RemoteSignerHandle::disconnect(&wrapper);

    let err = op
        .wait(Duration::from_millis(200))
        .expect_err("disconnect must surface as Err, not a timeout");
    assert!(
        matches!(err, SignerError::Rejected(ref m) if m.contains("disconnect")),
        "expected Rejected(disconnect…), got {err:?}"
    );
}

#[test]
fn start_nostrconnect_handshake_returns_well_formed_uri() {
    let (tx, _rx) = mpsc::channel::<ActorCommand>();
    let broker = BunkerBroker::new(tx);
    let uri = broker.start_nostrconnect_handshake("not-a-url".to_string());
    broker.cancel();

    assert!(
        uri.starts_with("nostrconnect://"),
        "uri must use the nostrconnect scheme: {uri:?}"
    );
    let after_scheme = uri.strip_prefix("nostrconnect://").unwrap();
    let (pubkey_hex, query) = after_scheme
        .split_once('?')
        .expect("uri must carry a query string");
    assert_eq!(pubkey_hex.len(), 64, "client pubkey must be 64 hex chars");
    assert!(
        pubkey_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "client pubkey must be hex: {pubkey_hex:?}"
    );

    let relay_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("relay="))
        .expect("uri must carry a relay param");
    assert!(
        !relay_param.contains(':') && !relay_param.contains('/'),
        "relay param must be percent-encoded: {relay_param:?}"
    );

    let secret = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("secret="))
        .expect("uri must carry a secret param");
    assert_eq!(secret.len(), 16, "session secret is 16 chars");
    assert!(
        secret.chars().all(|c| c.is_ascii_alphanumeric()),
        "session secret must be alphanumeric: {secret:?}"
    );
    assert!(
        query.contains("name=Chirp"),
        "uri must name the app: {query:?}"
    );
    assert!(
        query.contains("perms="),
        "uri must request perms: {query:?}"
    );
}
