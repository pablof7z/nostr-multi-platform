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

// ─── Defect 3 — the inbound dispatcher thread must be joined on cancel ───────

/// `cancel()` must JOIN the steady-state inbound-dispatcher thread, not leak it.
///
/// Before the fix, `install_completed_signer` spawned the dispatcher with no
/// join handle stored on `ActiveSession`; `cancel()` joined only the handshake
/// thread, so the dispatcher thread was detached and leaked — one stuck thread
/// per session teardown under rapid reconnects.
///
/// We reproduce the exact lifecycle shape without a live bunker: a relay stub
/// whose `shutdown()` drops the `inbound_tx` sender (mirroring the real relay
/// client dropping its event-callback clone), a dispatcher thread blocked on
/// `inbound_rx.recv()` that flips an exit flag when it returns, and a session
/// with that handle stored in `dispatcher_thread`. After `cancel()` the thread
/// MUST have exited; the join inside `cancel()` guarantees it.
#[test]
fn cancel_joins_inbound_dispatcher_thread_no_leak() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use crate::relay_client::{RelayClient, RelayError};
    use crate::transport::BrokerTransport;
    use nostr::Keys;

    // Relay stub: `shutdown()` drops the inbound sender it was handed, exactly
    // as the production relay client drops its event-callback `inbound_tx`
    // clone once `Pool::shutdown` joins the relay's own dispatcher.
    struct SenderDroppingRelay {
        inbound_tx: Mutex<Option<mpsc::Sender<serde_json::Value>>>,
    }
    impl std::fmt::Debug for SenderDroppingRelay {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SenderDroppingRelay").finish_non_exhaustive()
        }
    }
    impl RelayClient for SenderDroppingRelay {
        fn send(&self, _frame: String) -> Result<(), RelayError> {
            Ok(())
        }
        fn shutdown(&self) {
            // Drop the sender → the dispatcher's blocking recv() returns Err.
            if let Ok(mut slot) = self.inbound_tx.lock() {
                *slot = None;
            }
        }
    }

    let (broker, _rx) = test_broker();

    let (inbound_tx, inbound_rx) = mpsc::channel::<serde_json::Value>();
    let exited = Arc::new(AtomicBool::new(false));
    let exited_for_thread = Arc::clone(&exited);

    // Dispatcher thread blocked on recv — the exact production shape.
    let dispatcher = std::thread::Builder::new()
        .name("test-inbound-dispatch".to_string())
        .spawn(move || {
            while inbound_rx.recv().is_ok() {}
            exited_for_thread.store(true, Ordering::Release);
        })
        .expect("spawn dispatcher");

    let relay: Arc<dyn RelayClient> = Arc::new(SenderDroppingRelay {
        inbound_tx: Mutex::new(Some(inbound_tx)),
    });

    // Install a fully-formed session whose dispatcher_thread carries the handle.
    {
        let mut guard = broker.active.lock().expect("active lock");
        *guard = Some(ActiveSession {
            relay: Arc::clone(&relay),
            cancel: Arc::new(AtomicBool::new(false)),
            handshake_thread: None,
            dispatcher_thread: Some(dispatcher),
            transport: BrokerTransport::new(
                relay,
                Keys::generate(),
                Keys::generate().public_key(),
            ),
            signer: Mutex::new(None),
        });
    }

    // cancel() calls relay.shutdown() (drops the sender) then joins the
    // dispatcher. If the handle were leaked (pre-fix), the thread would still
    // be parked on recv() and `exited` would remain false.
    broker.cancel();

    assert!(
        exited.load(Ordering::Acquire),
        "cancel() must join the inbound dispatcher thread; it leaked (still parked on recv)"
    );
}

fn test_broker() -> (Arc<BunkerBroker>, mpsc::Receiver<BrokerEvent>) {
    let (tx, rx) = mpsc::channel::<BrokerEvent>();
    let broker = BunkerBroker::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    (broker, rx)
}
