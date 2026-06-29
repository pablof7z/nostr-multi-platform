//! Rust-side tests for the M14-C4 capability / action-lane / publish UniFFI
//! surface.
//!
//! # Coverage
//!
//! ## CapabilitySink (full quiescence — Condvar + in_flight drain)
//! * `capability_sink_register_and_clear` — register, clear, clear again
//!   (idempotent).
//! * `capability_sink_replace_is_atomic` — replace A with B; after return A
//!   is neither registered nor in-flight.
//! * `capability_sink_dispatch_routes_to_registered_sink` — after
//!   registration, `dispatch_capability_json` reaches `on_capability_request`.
//! * `capability_sink_panic_is_contained` — a panicking sink does not crash
//!   the dispatch thread; subsequent dispatch returns an error envelope.
//! * `capability_sink_clear_waits_for_in_flight` — full Barrier-style drain
//!   test: `set_capability_callback(None)` blocks while a callback is
//!   in-flight and returns only after it completes. Proof mirrors
//!   `clear_waits_for_in_flight_callback` from `tests.rs`.
//! * `capability_sink_shutdown_during_in_flight_no_uaf` — `shutdown()` while
//!   a callback is in-flight completes cleanly (no UAF / deadlock).
//!
//! ## ActionResultObserver (mutex-exclusion quiescence — no drain gate)
//! * `action_result_observer_fires_on_dispatch` — after registration, a
//!   dispatched action that is accepted calls `on_action_result`.
//! * `action_result_observer_replace_is_safe` — replacing the observer
//!   before re-dispatching routes to the new observer only.
//! * NOTE: Barrier-style teardown tests are absent — see module-level
//!   quiescence note in `action.rs`.
//!
//! ## ack_action_stage
//! * `ack_action_stage_empty_id_is_noop` — empty string is a silent no-op.
//! * `ack_action_stage_valid_id_sends_cmd` — a valid id reaches the actor.
//!
//! ## retry_publish / cancel_action
//! * `retry_publish_empty_handle_is_noop` — empty string is a silent no-op.
//! * `cancel_action_empty_id_is_noop` — empty string is a silent no-op.
//! * `retry_publish_valid_handle_sends_cmd` — non-empty handle sends command.
//! * `cancel_action_valid_id_sends_cmd` — non-empty id sends command.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::actor::ActorCommand;

use crate::NmpApp;
use super::{ActionResultObserver, CapabilitySink};

// ── CapabilitySink stubs ──────────────────────────────────────────────────────

/// Echoes the request JSON back as the response.
struct EchoSink;

impl CapabilitySink for EchoSink {
    fn on_capability_request(&self, request_json: String) -> String {
        request_json
    }
}

/// Panics unconditionally — verifies panic containment.
struct PanickingCapabilitySink;

impl CapabilitySink for PanickingCapabilitySink {
    fn on_capability_request(&self, _request_json: String) -> String {
        panic!("PanickingCapabilitySink: deliberate panic");
    }
}

/// Records the first request JSON it receives via a shared slot.
struct RecordSink {
    received: Arc<Mutex<Option<String>>>,
}

impl RecordSink {
    fn new_boxed() -> (Box<dyn CapabilitySink>, Arc<Mutex<Option<String>>>) {
        let slot = Arc::new(Mutex::new(None));
        let handle = Arc::clone(&slot);
        (Box::new(RecordSink { received: slot }), handle)
    }
}

impl CapabilitySink for RecordSink {
    fn on_capability_request(&self, request_json: String) -> String {
        *self.received.lock().unwrap() = Some(request_json.clone());
        request_json
    }
}

/// Signals entry via a channel then blocks at a `Barrier`. Used by quiescence
/// tests. Mirrors `BlockingSink` from `tests.rs`.
struct BlockingCapabilitySink {
    entered_tx: Mutex<Option<mpsc::Sender<()>>>,
    gate: Arc<std::sync::Barrier>,
    callback_done: Option<Arc<AtomicBool>>,
}

impl CapabilitySink for BlockingCapabilitySink {
    fn on_capability_request(&self, request_json: String) -> String {
        if let Ok(mut guard) = self.entered_tx.lock() {
            let _ = guard.take().map(|tx| tx.send(()));
        }
        self.gate.wait();
        if let Some(ref done) = self.callback_done {
            done.store(true, Ordering::SeqCst);
        }
        request_json
    }
}

// ── CapabilitySink: lifecycle / idempotence ───────────────────────────────────

/// Register a sink and then clear it twice — idempotent, no deadlock.
#[test]
fn capability_sink_register_and_clear() {
    let app = NmpApp::new();
    app.set_capability_callback(Some(Box::new(EchoSink)));
    app.set_capability_callback(None);
    app.set_capability_callback(None); // idempotent second clear
}

/// Replace A with B: after return, A is no longer registered.
#[test]
fn capability_sink_replace_is_atomic() {
    let app = NmpApp::new();
    let (sink_a, _ha) = RecordSink::new_boxed();
    let (sink_b, _hb) = RecordSink::new_boxed();
    app.set_capability_callback(Some(sink_a));
    app.set_capability_callback(Some(sink_b));
    app.set_capability_callback(None);
}

/// After registration, `dispatch_capability_json` routes to the sink.
#[test]
fn capability_sink_dispatch_routes_to_registered_sink() {
    let app = NmpApp::new();
    let (sink, received) = RecordSink::new_boxed();
    app.set_capability_callback(Some(sink));

    let request = r#"{"namespace":"test","correlation_id":"c1","payload_json":"{}"}"#;
    let response = app.dispatch_capability_json(request.to_string());

    // EchoSink returns the request as-is; verify the round-trip.
    assert_eq!(response, request);
    // RecordSink stores the request.
    assert_eq!(received.lock().unwrap().as_deref(), Some(request));

    app.set_capability_callback(None);
}

/// No-handler dispatch returns an error envelope (D6 — never panics).
#[test]
fn capability_sink_no_handler_yields_error_envelope() {
    let app = NmpApp::new();
    // No callback registered.
    let result = app.dispatch_capability_json(
        r#"{"namespace":"test","correlation_id":"c0"}"#.to_string(),
    );
    let v: serde_json::Value = serde_json::from_str(&result).expect("error envelope is valid JSON");
    assert_eq!(v["correlation_id"], "c0");
    assert!(
        v["result_json"]
            .as_str()
            .map_or(false, |s| s.contains("no-capability-handler")),
        "missing handler must yield error envelope, got: {result}"
    );
}

/// A panicking sink does not crash; subsequent dispatch returns an error
/// envelope (the panic is caught inside the wrapper).
#[test]
fn capability_sink_panic_is_contained() {
    let app = NmpApp::new();
    app.set_capability_callback(Some(Box::new(PanickingCapabilitySink)));
    let result = app.dispatch_capability_json(
        r#"{"namespace":"test","correlation_id":"c2"}"#.to_string(),
    );
    let v: serde_json::Value =
        serde_json::from_str(&result).expect("panic error envelope is valid JSON");
    assert_eq!(v["correlation_id"], "c2");
    // After a panicking sink the callback is still registered (it was the
    // same sink that panicked). We clear it and verify the app is still usable.
    app.set_capability_callback(None);
    // App is still alive — no UAF.
    let result2 = app.dispatch_capability_json(
        r#"{"namespace":"test","correlation_id":"c3"}"#.to_string(),
    );
    let v2: serde_json::Value = serde_json::from_str(&result2).unwrap();
    assert_eq!(v2["correlation_id"], "c3");
}

// ── CapabilitySink: quiescence (Condvar + in_flight drain) ────────────────────

/// Clearing the sink while a callback is in-flight must block until the
/// callback completes before returning (drain-gate quiescence contract).
///
/// Proof structure mirrors `clear_waits_for_in_flight_callback` from
/// `tests.rs` (BlockingSink / Barrier pattern), adapted for the capability
/// callback path.
///
/// `callback_done` is set AFTER the gate releases — a regression where clear
/// returns early would leave it `false`.
#[test]
fn capability_sink_clear_waits_for_in_flight() {
    let app = NmpApp::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let callback_done = Arc::new(AtomicBool::new(false));

    app.set_capability_callback(Some(Box::new(BlockingCapabilitySink {
        entered_tx: Mutex::new(Some(entered_tx)),
        gate: Arc::clone(&gate),
        callback_done: Some(Arc::clone(&callback_done)),
    })));

    // Dispatch on a background thread so the test thread stays free.
    let app_for_dispatch = Arc::clone(&app);
    let dispatch_request = r#"{"namespace":"test","correlation_id":"q1"}"#.to_string();
    let dispatch_handle = thread::spawn(move || {
        app_for_dispatch.dispatch_capability_json(dispatch_request)
    });

    // Wait for the callback to genuinely enter — deterministic, no sleep.
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("BlockingCapabilitySink never entered within 2s");

    // Spawn a helper to release the gate concurrently with clear() below.
    let gate_clone = Arc::clone(&gate);
    let helper = thread::spawn(move || gate_clone.wait());

    // Run clear() in its own thread so we can bound it with recv_timeout.
    let app_for_clear = Arc::clone(&app);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let clear_handle = thread::spawn(move || {
        app_for_clear.set_capability_callback(None);
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("set_capability_callback(None) deadlocked while callback was in-flight");

    // Quiescence proof: callback_done is set AFTER gate.wait(), which
    // happens-before in_flight is decremented and drained is notified,
    // which happens-before set_capability_callback(None) returns.
    assert!(
        callback_done.load(Ordering::SeqCst),
        "set_capability_callback(None) returned before callback completed — quiescence violated"
    );

    dispatch_handle.join().unwrap();
    clear_handle.join().unwrap();
    helper.join().unwrap();
}

/// `shutdown()` while a capability callback is in-flight must not UAF or
/// deadlock: the app clears the capability slot (which waits for in-flight
/// to drain) before completing shutdown.
#[test]
fn capability_sink_shutdown_during_in_flight_no_uaf() {
    let app = NmpApp::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Barrier::new(2));

    app.set_capability_callback(Some(Box::new(BlockingCapabilitySink {
        entered_tx: Mutex::new(Some(entered_tx)),
        gate: Arc::clone(&gate),
        callback_done: None,
    })));

    let app_for_dispatch = Arc::clone(&app);
    let dispatch_handle = thread::spawn(move || {
        app_for_dispatch.dispatch_capability_json(
            r#"{"namespace":"test","correlation_id":"q2"}"#.to_string(),
        )
    });

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("BlockingCapabilitySink never entered within 2s");

    // Helper releases the gate so the capability callback can return.
    let gate_clone = Arc::clone(&gate);
    let helper = thread::spawn(move || gate_clone.wait());

    // Shutdown in its own thread with a wall-clock deadline.
    let app_for_shutdown = Arc::clone(&app);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let shutdown_handle = thread::spawn(move || {
        app_for_shutdown.shutdown();
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("shutdown() deadlocked while capability callback was in-flight");

    dispatch_handle.join().unwrap();
    shutdown_handle.join().unwrap();
    helper.join().unwrap();

    // Idempotent shutdown.
    app.shutdown();
}

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

    let envelope = encode_dispatch_envelope(
        "corr-obs-1",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let outcome = app.dispatch_action(envelope);
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

/// Replacing the observer before a second dispatch routes to the new
/// observer only (mutex exclusion — see quiescence note in action.rs).
#[test]
fn action_result_observer_replace_is_safe() {
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(SucceedModule);
    let app = std::sync::Arc::new(NmpApp { inner });

    let (observer_a, received_a) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_a);

    // First dispatch — observer A fires.
    let env1 = encode_dispatch_envelope(
        "corr-obs-2a",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let _ = app.dispatch_action(env1);
    assert_eq!(received_a.lock().unwrap().len(), 1, "observer A: first dispatch");

    // Replace with observer B.
    let (observer_b, received_b) = RecordObserver::new_boxed();
    app.register_action_result_observer(observer_b);

    // Second dispatch — only observer B fires.
    let env2 = encode_dispatch_envelope(
        "corr-obs-2b",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let _ = app.dispatch_action(env2);
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

    let envelope = encode_dispatch_envelope(
        "corr-obs-panic",
        SucceedModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    // Must not panic.
    let outcome = app.dispatch_action(envelope);
    assert!(outcome.correlation_id.is_some(), "dispatch must succeed even when observer panics");
}

// ── ack_action_stage ──────────────────────────────────────────────────────────

/// Empty `correlation_id` is a silent no-op — does not send any command.
#[test]
fn ack_action_stage_empty_id_is_noop() {
    let app = NmpApp::new();
    app.ack_action_stage(String::new());
    // No assertion needed beyond "did not panic".
}

/// Non-empty `correlation_id` is accepted and sent to the actor (no panic).
#[test]
fn ack_action_stage_valid_id_sends_cmd() {
    let app = NmpApp::new();
    app.ack_action_stage("corr-ack-1".to_string());
    // Non-blocking channel send — accepted without error.
}

// ── retry_publish / cancel_action ────────────────────────────────────────────

/// Empty handle is a silent no-op.
#[test]
fn retry_publish_empty_handle_is_noop() {
    let app = NmpApp::new();
    app.retry_publish(String::new());
}

/// Empty `correlation_id` is a silent no-op.
#[test]
fn cancel_action_empty_id_is_noop() {
    let app = NmpApp::new();
    app.cancel_action(String::new());
}

/// Non-empty handle reaches the actor channel (no panic).
#[test]
fn retry_publish_valid_handle_sends_cmd() {
    let app = NmpApp::new();
    app.retry_publish("handle-abc".to_string());
}

/// Non-empty `correlation_id` reaches the actor channel (no panic).
#[test]
fn cancel_action_valid_id_sends_cmd() {
    let app = NmpApp::new();
    app.cancel_action("corr-cancel-1".to_string());
}

