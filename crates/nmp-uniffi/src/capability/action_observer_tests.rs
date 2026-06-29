//! Rust-side tests for the UniFFI `ActionResultObserver` push observer.
//!
//! Split out of `capability/tests.rs` (file-size ceiling). Covers register/
//! fire parity AND the M14-C-tail (#2429) drain/teardown contract now that
//! `ActionRegistry` has a `ResultObserverGate`:
//! * `action_result_observer_fires_on_dispatch` — delivery on accept.
//! * `action_result_observer_replace_is_safe` — replacement routes correctly.
//! * `action_result_observer_panic_is_contained` — a throwing observer is caught.
//! * `action_result_observer_register_clear_idempotent` — clear unregisters.
//! * `action_result_observer_clear_waits_for_in_flight` — clear drains an
//!   in-flight delivery (deterministic; the negative `recv_timeout` flips to a
//!   failing assertion under a drain regression).
//! * `action_result_observer_shutdown_during_in_flight_no_uaf` — teardown is
//!   deadlock/UAF-free while a delivery is in-flight.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nmp_core::actor::ActorCommand;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};

use super::ActionResultObserver;
use crate::NmpApp;

// ── ActionResultObserver stubs ────────────────────────────────────────────────

/// Records the first JSON it receives.
struct RecordObserver {
    received: Arc<Mutex<Vec<String>>>,
}

impl RecordObserver {
    fn new_boxed() -> (Box<dyn ActionResultObserver>, Arc<Mutex<Vec<String>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let handle = Arc::clone(&received);
        (Box::new(RecordObserver { received }), handle)
    }
}

impl ActionResultObserver for RecordObserver {
    fn on_action_result(&self, result_json: String) {
        self.received.lock().unwrap().push(result_json);
    }
}

// ── ActionModule for observer tests ──────────────────────────────────────────

/// Minimal action module that always succeeds — used to trigger the observer.
struct SucceedModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]

impl ActionModule for SucceedModule {
    const NAMESPACE: &'static str = "test.uniffi_c4.succeed"; // doctrine-allow: action_namespace — test fixture
    type Action = serde_json::Value;

    fn decode_payload(
        _bytes: &[u8],
    ) -> Option<Result<Self::Action, nmp_core::substrate::ActionPayloadDecodeError>> {
        Some(Ok(serde_json::Value::Null))
    }

    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Observer that signals entry via `started_tx`, blocks on `release_rx`
/// (main-thread controlled), then sets `done` AFTER release. Used by the
/// drain/teardown tests. Mirrors `BlockingCapabilitySink`.
struct BlockingObserver {
    started_tx: Mutex<Option<mpsc::SyncSender<()>>>,
    release_rx: Mutex<mpsc::Receiver<()>>,
    done: Arc<AtomicBool>,
}

impl ActionResultObserver for BlockingObserver {
    fn on_action_result(&self, _result_json: String) {
        if let Ok(mut g) = self.started_tx.lock() {
            let _ = g.take().map(|tx| tx.send(()));
        }
        let _ = self.release_rx.lock().unwrap().recv();
        self.done.store(true, Ordering::SeqCst);
    }
}

fn succeed_envelope(correlation_id: &str) -> Vec<u8> {
    encode_dispatch_envelope(
        correlation_id,
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    )
}

// ── ActionResultObserver tests ────────────────────────────────────────────────

/// After registration, a successful dispatch calls `on_action_result` with a
/// JSON string containing the `correlation_id`.
#[test]
fn action_result_observer_fires_on_dispatch() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    let (observer, received) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer);

    let outcome = app.dispatch_action(succeed_envelope("corr-obs-1"));
    assert!(outcome.correlation_id.is_some(), "dispatch must succeed");

    // The observer fires synchronously on the dispatch thread.
    let calls = received.lock().unwrap();
    assert_eq!(calls.len(), 1, "observer must fire exactly once");
    let v: serde_json::Value = serde_json::from_str(&calls[0]).unwrap();
    assert_eq!(
        v["correlation_id"].as_str(),
        Some("corr-obs-1"),
        "observer must carry the correlation_id"
    );
}

/// Replacing the observer before a second dispatch routes to the new observer
/// only, and drains the previous observer before returning (M14-C-tail).
#[test]
fn action_result_observer_replace_is_safe() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    let (observer_a, received_a) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_a);

    let _ = app.dispatch_action(succeed_envelope("corr-obs-2a"));
    assert_eq!(received_a.lock().unwrap().len(), 1, "observer A: first dispatch");

    // Replace with observer B.
    let (observer_b, received_b) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_b);

    let _ = app.dispatch_action(succeed_envelope("corr-obs-2b"));
    assert_eq!(
        received_a.lock().unwrap().len(),
        1,
        "observer A must not fire after replacement"
    );
    assert_eq!(received_b.lock().unwrap().len(), 1, "observer B: second dispatch");
}

/// A panicking observer does not crash the dispatch thread.
#[test]
fn action_result_observer_panic_is_contained() {
    struct PanickingObserver;
    impl ActionResultObserver for PanickingObserver {
        fn on_action_result(&self, _result_json: String) {
            panic!("PanickingObserver: deliberate panic");
        }
    }

    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    app.register_action_result_observer(Box::new(PanickingObserver));

    // Must not panic.
    let outcome = app.dispatch_action(succeed_envelope("corr-obs-panic"));
    assert!(
        outcome.correlation_id.is_some(),
        "dispatch must succeed even when observer panics"
    );
}

/// Register → clear → clear: idempotent, ends unregistered (no delivery fires
/// after clear).
#[test]
fn action_result_observer_register_clear_idempotent() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    let (observer, received) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer);
    app.clear_action_result_observer();
    app.clear_action_result_observer(); // idempotent

    // After clear, a successful dispatch delivers to nobody.
    let outcome = app.dispatch_action(succeed_envelope("corr-clear-1"));
    assert!(outcome.correlation_id.is_some(), "dispatch still succeeds");
    assert!(
        received.lock().unwrap().is_empty(),
        "cleared observer must not fire"
    );
}

/// THE teardown drain proof: `clear_action_result_observer()` must block while
/// a delivery is in-flight and return only after it completes.
#[test]
fn action_result_observer_clear_waits_for_in_flight() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let done = Arc::new(AtomicBool::new(false));
    app.register_action_result_observer(Box::new(BlockingObserver {
        started_tx: Mutex::new(Some(started_tx)),
        release_rx: Mutex::new(release_rx),
        done: Arc::clone(&done),
    }));

    // Dispatch on a background thread — deliver_result runs the observer
    // synchronously on that thread, where it blocks.
    let app_for_dispatch = Arc::clone(&app);
    let dispatch = thread::spawn(move || {
        app_for_dispatch.dispatch_action(succeed_envelope("corr-drain-1"))
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("observer should start");

    let (clear_done_tx, clear_done_rx) = mpsc::sync_channel(1);
    let app_for_clear = Arc::clone(&app);
    let clear = thread::spawn(move || {
        app_for_clear.clear_action_result_observer();
        clear_done_tx.send(()).unwrap();
    });

    // Negative check: clear must NOT return while the delivery is mid-flight.
    assert!(
        clear_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "clear_action_result_observer() returned while delivery was mid-flight — quiescence violated"
    );
    release_tx.send(()).unwrap();
    clear_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear should return after the delivery drains");

    clear.join().unwrap();
    dispatch.join().unwrap();
    assert!(
        done.load(Ordering::SeqCst),
        "observer must have completed before clear returned"
    );
}

/// `shutdown()` while an action-result delivery is in-flight must not UAF or
/// deadlock: the dispatch thread holds an `Arc<NmpApp>`, so teardown waits for
/// it; the app stays usable and shutdown is idempotent.
#[test]
fn action_result_observer_shutdown_during_in_flight_no_uaf() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });
    app.start(256, 4);

    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let done = Arc::new(AtomicBool::new(false));
    app.register_action_result_observer(Box::new(BlockingObserver {
        started_tx: Mutex::new(Some(started_tx)),
        release_rx: Mutex::new(release_rx),
        done: Arc::clone(&done),
    }));

    let app_for_dispatch = Arc::clone(&app);
    let dispatch = thread::spawn(move || {
        app_for_dispatch.dispatch_action(succeed_envelope("corr-drain-2"))
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("observer should start");

    // Clear (drains) in its own thread, bounded; release the delivery so it
    // can complete.
    let (clear_done_tx, clear_done_rx) = mpsc::sync_channel(1);
    let app_for_clear = Arc::clone(&app);
    let clear = thread::spawn(move || {
        app_for_clear.clear_action_result_observer();
        clear_done_tx.send(()).unwrap();
    });
    release_tx.send(()).unwrap();
    clear_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear_action_result_observer() deadlocked under teardown");

    clear.join().unwrap();
    dispatch.join().unwrap();
    assert!(done.load(Ordering::SeqCst));

    app.shutdown();
    app.shutdown(); // idempotent
}
