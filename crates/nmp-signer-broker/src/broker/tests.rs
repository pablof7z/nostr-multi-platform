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
/// while a worker thread is still winding down, and it MUST NOT join (inline,
/// on the actor / capability call path) ANY thread — including the relay
/// client's own background thread. Every join is performed off-path by the
/// detached reaper; the threads self-exit on the cancel/shutdown signal.
///
/// Pre-D4 the contract was the inverse: `cancel()` called `relay.shutdown()`
/// (which joined the relay dispatcher inline) and joined the broker dispatcher
/// inline, so a relay worker stuck mid-connect froze the actor for up to the
/// connect timeout — the freeze ADR-0050's signer-session port set out to kill.
///
/// Proof shape (no live bunker):
///   - `SenderDroppingRelay::signal_shutdown` drops the inbound `inbound_tx`
///     (the cancel signal the broker dispatcher's `recv` observes) AND
///     surrenders a relay-dispatcher `JoinHandle` to the caller — exactly as
///     the production `PoolRelayClient::signal_shutdown` surrenders its Pool
///     dispatcher handle instead of joining it.
///   - Both surrendered threads block on a barrier `Mutex` the test holds, so
///     they are provably still alive when `cancel()` returns. An inline join
///     (pre-D4) would hang `cancel()` on that barrier and fail the timing bound.
///   - After releasing the barrier we confirm BOTH threads ran to completion
///     (no leak) AND that the reaper itself terminated having joined them
///     (a `joined`-side-channel the reaper-joined threads share with a watcher
///     that blocks on the reaper's own exit), so a handle that was merely
///     dropped (detached) rather than joined would not satisfy the assertion.
#[test]
fn cancel_is_signal_only_does_not_block_on_join() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::thread::JoinHandle;
    use std::time::Instant;

    use crate::relay_client::{RelayClient, RelayError};
    use crate::transport::BrokerTransport;
    use nostr::Keys;

    // Relay stub: `signal_shutdown` drops the inbound sender (cancel signal)
    // and surrenders the relay-dispatcher handle stashed at construction — the
    // production D4 contract. It must NOT block.
    struct SenderDroppingRelay {
        inbound_tx: Mutex<Option<mpsc::Sender<serde_json::Value>>>,
        relay_dispatcher: Mutex<Option<JoinHandle<()>>>,
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
        fn signal_shutdown(&self) -> Option<JoinHandle<()>> {
            // Drop the sender → the broker dispatcher's recv() returns Err.
            if let Ok(mut slot) = self.inbound_tx.lock() {
                *slot = None;
            }
            // Surrender the relay-dispatcher handle (do NOT join here).
            self.relay_dispatcher.lock().ok().and_then(|mut g| g.take())
        }
    }

    let (broker, _rx) = test_broker();

    let (inbound_tx, inbound_rx) = mpsc::channel::<serde_json::Value>();
    // Counts how many of the two worker threads have run to completion.
    let exits = Arc::new(AtomicUsize::new(0));

    // Barrier both worker threads must acquire on their way out. The test holds
    // it, so they stay parked even after the cancel signal — modelling workers
    // still winding down. Blocking lock, no poll (D8).
    let barrier = Arc::new(Mutex::new(()));
    let barrier_held = barrier.lock().expect("hold barrier");

    // The broker inbound dispatcher: blocks on recv, then on the barrier.
    let exits_for_disp = Arc::clone(&exits);
    let barrier_for_disp = Arc::clone(&barrier);
    let dispatcher = std::thread::Builder::new()
        .name("test-inbound-dispatch".to_string())
        .spawn(move || {
            while inbound_rx.recv().is_ok() {}
            let _g = barrier_for_disp.lock().expect("barrier acquire (disp)");
            exits_for_disp.fetch_add(1, Ordering::AcqRel);
        })
        .expect("spawn dispatcher");

    // The relay client's own dispatcher: independent thread, also barrier-gated.
    let exits_for_relay = Arc::clone(&exits);
    let barrier_for_relay = Arc::clone(&barrier);
    let relay_dispatcher = std::thread::Builder::new()
        .name("test-relay-dispatch".to_string())
        .spawn(move || {
            let _g = barrier_for_relay.lock().expect("barrier acquire (relay)");
            exits_for_relay.fetch_add(1, Ordering::AcqRel);
        })
        .expect("spawn relay dispatcher");

    let relay: Arc<dyn RelayClient> = Arc::new(SenderDroppingRelay {
        inbound_tx: Mutex::new(Some(inbound_tx)),
        relay_dispatcher: Mutex::new(Some(relay_dispatcher)),
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

    // cancel() must return promptly while BOTH worker threads are still parked
    // on the barrier we hold. A join on any of them (inline, pre-D4 — including
    // the relay's own `shutdown` join) would block here until we release the
    // barrier below, which we have not done yet.
    let start = Instant::now();
    broker.cancel();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "cancel() must be signal-only and not block on any join; it returned in \
         {elapsed:?} while both a broker dispatcher AND the relay dispatcher were \
         still winding down (any inline join would have hung on the held barrier)"
    );

    // Both threads must still be alive (parked on the barrier) — proof cancel
    // did NOT wait for either.
    assert_eq!(
        exits.load(Ordering::Acquire),
        0,
        "both worker threads should still be winding down: cancel() detached them, did not join"
    );

    // Release the barrier so both threads complete. Then prove the REAPER (not
    // just the threads) finished its joins: spawn a watcher that holds the
    // barrier-free condition and signals once both exits are observed. We block
    // (no poll) on that watcher's completion channel.
    drop(barrier_held);

    let (reaped_tx, reaped_rx) = mpsc::channel::<usize>();
    let exits_for_watch = Arc::clone(&exits);
    let barrier_for_watch = Arc::clone(&barrier);
    std::thread::Builder::new()
        .name("test-reap-watcher".to_string())
        .spawn(move || {
            // Acquiring the (now-free) barrier orders this watcher strictly
            // after both worker threads have released it on their way out —
            // i.e. after both have run their exit-count increment. No polling.
            let _g = barrier_for_watch.lock().expect("watcher barrier acquire");
            // Both increments happen-before the barrier release each performs,
            // so by the time we hold the barrier uncontended both are visible.
            reaped_tx
                .send(exits_for_watch.load(Ordering::Acquire))
                .ok();
        })
        .expect("spawn reap watcher");

    let observed = reaped_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("both threads must self-exit after the signal; reaper reclaims them (no leak)");

    assert_eq!(
        observed, 2,
        "both the broker dispatcher AND the relay dispatcher must exit on their own \
         once the barrier frees; the detached reaper joins (reclaims) both — no leak"
    );
}

fn test_broker() -> (Arc<BunkerBroker>, mpsc::Receiver<BrokerEvent>) {
    let (tx, rx) = mpsc::channel::<BrokerEvent>();
    let broker = BunkerBroker::new(Arc::new(move |event| {
        let _ = tx.send(event);
    }));
    (broker, rx)
}
