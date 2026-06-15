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

// ─── Workstream D4 — cancel is signal-only / detached, never a join on-path ──

/// `cancel()` must be **signal-only / detached**: it returns promptly even
/// while a worker thread is still winding down, and it MUST NOT join inline on
/// the actor / capability call path. The thread later self-exits on the signal
/// and the background reaper reclaims it — no leak.
///
/// Pre-D4 the contract was the inverse (and this test enshrined it): `cancel()`
/// joined the dispatcher (and handshake) handle inline, so a worker stuck mid
/// operation froze the actor for up to the relay connect timeout — the 10s
/// freeze ADR-0050's signer-session port set out to kill.
///
/// Shape (no live bunker): a relay stub whose `shutdown()` drops the inbound
/// `inbound_tx` (mirroring the real relay dropping its event-callback clone —
/// the cancel signal), and a dispatcher thread that, *after* observing that
/// signal (recv → Err), blocks on a barrier `Mutex` the test holds. So the
/// thread is provably still alive when `cancel()` returns. If `cancel()` joined
/// inline it would block on that barrier and the timing assertion would fail.
/// Then we release the barrier and block (no poll) on a completion channel the
/// thread sends right before exit, proving it self-exits and is reclaimed.
#[test]
fn cancel_is_signal_only_does_not_block_on_join() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    use crate::relay_client::{RelayClient, RelayError};
    use crate::transport::BrokerTransport;
    use nostr::Keys;

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
            // Drop the sender → the dispatcher's blocking recv() returns Err
            // (the cancel signal the thread observes).
            if let Ok(mut slot) = self.inbound_tx.lock() {
                *slot = None;
            }
        }
    }

    let (broker, _rx) = test_broker();

    let (inbound_tx, inbound_rx) = mpsc::channel::<serde_json::Value>();
    let exited = Arc::new(AtomicBool::new(false));
    let exited_for_thread = Arc::clone(&exited);

    // Barrier the dispatcher must acquire on its way out. The test holds it,
    // so the thread stays parked even after the cancel signal — modelling a
    // worker still winding down a slow operation. Blocking lock, no poll (D8).
    let barrier = Arc::new(Mutex::new(()));
    let barrier_held = barrier.lock().expect("hold barrier");
    let barrier_for_thread = Arc::clone(&barrier);

    // Completion channel: the thread sends `()` as its final act so the test
    // can block (recv) on its exit without polling.
    let (done_tx, done_rx) = mpsc::channel::<()>();

    let dispatcher = std::thread::Builder::new()
        .name("test-inbound-dispatch".to_string())
        .spawn(move || {
            while inbound_rx.recv().is_ok() {}
            // Signal observed (recv → Err). Still winding down: block on the
            // barrier the test holds. This blocks (parks) — no busy-wait.
            let _g = barrier_for_thread.lock().expect("barrier acquire");
            exited_for_thread.store(true, Ordering::Release);
            let _ = done_tx.send(());
        })
        .expect("spawn dispatcher");

    let relay: Arc<dyn RelayClient> = Arc::new(SenderDroppingRelay {
        inbound_tx: Mutex::new(Some(inbound_tx)),
    });

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

    // cancel() signals (relay.shutdown drops the sender) and detaches the
    // handle to the reaper. It MUST return promptly while the dispatcher is
    // still parked on the barrier we hold. A join-on-path (pre-D4) would block
    // here until we release `barrier_held` below — which we have not done yet.
    let start = Instant::now();
    broker.cancel();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "cancel() must be signal-only and not block on a join; it returned in \
         {elapsed:?} while a dispatcher was still winding down (an inline join \
         would have hung on the held barrier)"
    );

    // The thread must still be alive (parked on the barrier) — proof cancel did
    // NOT wait for it.
    assert!(
        !exited.load(Ordering::Acquire),
        "dispatcher should still be winding down: cancel() detached it, did not join it"
    );

    // Release the barrier; the dispatcher proceeds, sets `exited`, sends on the
    // completion channel, and returns. The reaper joins it (no leak). Block on
    // the completion channel (no poll) to observe the self-exit.
    drop(barrier_held);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("dispatcher must self-exit after the signal; reaper reclaims it (no leak)");

    assert!(
        exited.load(Ordering::Acquire),
        "dispatcher must exit on its own once the barrier frees; the reaper reclaims it"
    );
}

fn test_broker() -> (Arc<BunkerBroker>, mpsc::Receiver<BrokerEvent>) {
    let (tx, rx) = mpsc::channel::<BrokerEvent>();
    let broker = BunkerBroker::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    (broker, rx)
}
