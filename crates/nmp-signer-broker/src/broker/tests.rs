use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::BrokerEvent;

#[test]
fn new_and_cancel_are_noops_without_session() {
    let (broker, _rx) = test_broker();
    broker.cancel();
    broker.cancel();
}

#[test]
fn start_handshake_with_invalid_uri_emits_failed_progress() {
    let (broker, rx) = test_broker();
    broker.start_handshake("not-a-bunker-uri".to_string());

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("failed-progress event");
    assert!(
        matches!(
            event,
            BrokerEvent::Progress { ref stage, .. } if stage == "failed"
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

#[test]
fn start_nostrconnect_handshake_returns_well_formed_uri() {
    let (broker, _rx) = test_broker();
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
        query.contains("name=nmp"),
        "uri must carry a protocol-neutral client name (D0): {query:?}"
    );
    assert!(
        query.contains("perms="),
        "uri must request perms: {query:?}"
    );
}

fn test_broker() -> (Arc<BunkerBroker>, mpsc::Receiver<BrokerEvent>) {
    let (tx, rx) = mpsc::channel::<BrokerEvent>();
    let broker = BunkerBroker::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    (broker, rx)
}
