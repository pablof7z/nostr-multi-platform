//! Rust-side tests for the UniFFI `NmpApp` wrapper.
//!
//! Coverage (spec §"Tests the first PR must prove"):
//! * Sink register / clear / replace
//! * Clear waits for an in-flight callback (quiescence)
//! * Callback panic is contained
//! * Shutdown during an in-flight update → no UAF / deadlock
//! * Dispatch preserves correlation_id / error / code (covered by
//!   `nmp-native-runtime/src/action_dispatch_tests.rs` for the typed core;
//!   here we verify the wrapper surface passes through correctly)
//! * `start` / `configure` clamp parity with the C-ABI
//! * Generated Swift/Kotlin drift is deterministic (CI gate, not a unit test)
//!
//! # UniFFI 0.29 note on UpdateSink
//!
//! UniFFI 0.29 generates `Lift` for `Box<dyn Trait>`, not `Arc<dyn Trait>`,
//! for callback interfaces. Tests therefore use `Box<dyn UpdateSink>` directly
//! rather than wrapping in Arc first.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::actor::ActorCommand;

use super::{clamp_emit_hz, clamp_visible, NmpApp, UpdateSink};
use nmp_native_runtime::DEFAULT_EMIT_HZ;
use nmp_native_runtime::DEFAULT_VISIBLE_LIMIT;

// ── Sink stubs ────────────────────────────────────────────────────────────────

/// Minimal `UpdateSink` that records received frames via a shared counter.
///
/// The shared `Arc<Mutex<Vec<...>>>` handle is kept outside the Box so tests
/// can observe the count after passing ownership to `set_update_sink`.
struct RecordSink {
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl RecordSink {
    /// Returns a (Box<dyn UpdateSink>, shared_handle) pair.
    /// Pass the `Box` to `set_update_sink`; retain the handle for assertions.
    fn new_boxed() -> (Box<dyn UpdateSink>, Arc<Mutex<Vec<Vec<u8>>>>) {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let handle = Arc::clone(&frames);
        (Box::new(RecordSink { frames }), handle)
    }
}

impl UpdateSink for RecordSink {
    fn on_update(&self, frame: Vec<u8>) {
        self.frames.lock().unwrap().push(frame);
    }
}

/// `UpdateSink` that panics on the first call — verifies panic containment.
struct PanickingSink;

impl UpdateSink for PanickingSink {
    fn on_update(&self, _frame: Vec<u8>) {
        panic!("PanickingSink: deliberate panic in on_update");
    }
}

/// `UpdateSink` that signals callback entry via a channel and then blocks on a
/// `Barrier` until released. Used by the quiescence and shutdown-ordering tests.
///
/// `entered_tx` fires at most once (via `Option::take`) so concurrent
/// `on_update` calls do not overflow the channel. When `callback_done` is set,
/// it is stored to `true` AFTER the gate releases — tests assert this to prove
/// the callback ran to completion before `clear` / `shutdown` returned.
struct BlockingSink {
    entered_tx: Mutex<Option<mpsc::Sender<()>>>,
    gate: Arc<std::sync::Barrier>,
    callback_done: Option<Arc<AtomicBool>>,
}

impl UpdateSink for BlockingSink {
    fn on_update(&self, _frame: Vec<u8>) {
        // Signal entry deterministically — no sleep needed on the main thread.
        if let Ok(mut guard) = self.entered_tx.lock() {
            let _ = guard.take().map(|tx| tx.send(()));
        }
        // Block until the test releases us — simulates an in-flight callback.
        self.gate.wait();
        // Mark completion AFTER the gate so ordering assertions can prove that
        // clear / shutdown returned only after the callback finished.
        if let Some(ref done) = self.callback_done {
            done.store(true, Ordering::SeqCst);
        }
    }
}

// ── Clamp parity ──────────────────────────────────────────────────────────────

/// `visible_limit = 0` must map to `DEFAULT_VISIBLE_LIMIT` (100), matching
/// `nmp_app_start`'s clamping behaviour in `nmp-ffi`.
#[test]
fn clamp_visible_zero_yields_default() {
    assert_eq!(clamp_visible(0), DEFAULT_VISIBLE_LIMIT);
}

/// Non-zero `visible_limit` is clamped to [1, 500].
#[test]
fn clamp_visible_clamps_to_range() {
    assert_eq!(clamp_visible(1), 1);
    assert_eq!(clamp_visible(500), 500);
    assert_eq!(clamp_visible(999), 500); // above ceiling → 500
    assert_eq!(clamp_visible(200), 200);
}

/// `emit_hz = 0` must map to `DEFAULT_EMIT_HZ` (6 Hz), matching
/// `nmp_app_configure`'s clamping behaviour in `nmp-ffi`.
#[test]
fn clamp_emit_hz_zero_yields_default() {
    assert_eq!(clamp_emit_hz(0), DEFAULT_EMIT_HZ);
}

/// Non-zero `emit_hz` is clamped to [1, 12].
#[test]
fn clamp_emit_hz_clamps_to_range() {
    assert_eq!(clamp_emit_hz(1), 1);
    assert_eq!(clamp_emit_hz(12), 12);
    assert_eq!(clamp_emit_hz(99), 12); // above ceiling → 12
    assert_eq!(clamp_emit_hz(6), 6);
}

// ── Sink lifecycle ────────────────────────────────────────────────────────────

/// Setting a sink and then immediately clearing it (`None`) must leave the app
/// in a clean state (no listener). Idempotent — clearing twice is also safe.
#[test]
fn sink_register_and_clear() {
    let app = NmpApp::new();
    let (sink, _handle) = RecordSink::new_boxed();
    app.set_update_sink(Some(sink));
    app.set_update_sink(None); // clear
    app.set_update_sink(None); // idempotent second clear
    // No assertion needed beyond "did not deadlock or panic".
}

/// Replacing a sink atomically: after `set_update_sink(new_sink)` returns, the
/// previous sink is no longer invoked (quiescence guarantee).
#[test]
fn sink_replace_is_atomic() {
    let app = NmpApp::new();
    let (sink_a, _ha) = RecordSink::new_boxed();
    let (sink_b, _hb) = RecordSink::new_boxed();
    app.set_update_sink(Some(sink_a));
    // Replace with sink_b — after this returns, sink_a is guaranteed quiet.
    app.set_update_sink(Some(sink_b));
    app.set_update_sink(None);
    // If quiescence were broken, the drop of sink_a would potentially race
    // with an in-flight callback. The `set_update_sink` Condvar gate prevents
    // that; reaching here without a UAF is the pass condition.
}

/// A callback that panics must not kill the update-listener thread or the app.
/// The app must remain usable after a panicking callback.
///
/// # Panic-containment layering
///
/// Two independent `catch_unwind` guards exist:
///
/// 1. **Wrapper layer** (`lib.rs`): `catch_unwind` around `s.on_update(frame)`.
///    This is the primary guard for the cross-FFI case. When Swift or Kotlin
///    code throws / aborts, the exception is modelled as a Rust panic by the
///    generated glue; unwinding across an FFI boundary is undefined behaviour
///    in Rust, so catching it at the wrapper is the load-bearing defence against
///    UB propagating into the listener thread.
///
/// 2. **Runtime layer** (`app_ctor.rs`): `catch_unwind` around `listener(&update)`,
///    which wraps the entire wrapper closure. This is defence-in-depth — it
///    ensures a panic originating inside the wrapper itself (not just in the
///    foreign call) does not crash the listener thread.
///
/// In a pure-Rust test the runtime-layer guard would suffice; the wrapper guard
/// is a belt-and-suspenders contract specifically for cross-FFI use. The test
/// below exercises the combined effect of both layers.
#[test]
fn sink_panicking_callback_is_contained() {
    let app = NmpApp::new();
    app.set_update_sink(Some(Box::new(PanickingSink)));
    // The pre-start snapshot fires synchronously when a listener is installed
    // (passive_start.rs `emit_passive_prestart_snapshot`). If panic containment
    // were broken the test thread would observe a panic here.
    // Replace the panicking sink — this also exercises quiescence after panic.
    let (record, _handle) = RecordSink::new_boxed();
    app.set_update_sink(Some(record));
    app.set_update_sink(None);
}

// ── Quiescence / shutdown safety ──────────────────────────────────────────────

/// Clearing the sink while a callback is in flight must wait until the
/// callback completes before returning (quiescence contract).
///
/// # Proof structure
///
/// `BlockingSink` signals entry via a channel (deterministic — no sleep), then
/// blocks at a `Barrier`. The passive pre-start snapshot fires `on_update` on
/// registration, so the callback is genuinely in-flight by the time the main
/// thread receives the "entered" signal.
///
/// `callback_done` is set inside `on_update` AFTER the gate. The runtime
/// decrements `in_flight` only after the callback returns, then notifies the
/// `drained` Condvar. After `set_update_sink(None)` returns, `callback_done`
/// MUST be `true` — if clear returned early (quiescence broken), the callback
/// would still be at the gate and `callback_done` would be `false`.
///
/// A `recv_timeout` bounds the whole operation so a regression FAILS with an
/// explanatory message rather than hanging CI.
#[test]
fn clear_waits_for_in_flight_callback() {
    let app = NmpApp::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let callback_done = Arc::new(AtomicBool::new(false));

    app.set_update_sink(Some(Box::new(BlockingSink {
        entered_tx: Mutex::new(Some(entered_tx)),
        gate: Arc::clone(&gate),
        callback_done: Some(Arc::clone(&callback_done)),
    })));

    // Wait until on_update has entered — fully deterministic, no sleep.
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("passive pre-start snapshot never triggered on_update within 2s");

    // Spawn helper to release the gate concurrently with clear() below.
    // Barrier::new(2) handles any arrival order: if the helper arrives before
    // on_update it blocks; if on_update arrived first it is already waiting.
    // Either way, both parties proceed together once the helper calls wait().
    let gate_clone = Arc::clone(&gate);
    let helper = thread::spawn(move || gate_clone.wait());

    // Run clear() in its own thread so we can bound it with recv_timeout.
    let app_for_clear = Arc::clone(&app);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let clear_handle = thread::spawn(move || {
        app_for_clear.set_update_sink(None);
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("set_update_sink(None) deadlocked while on_update was in-flight");

    // Quiescence proof: `callback_done` is set inside on_update AFTER the gate,
    // which happens-before `in_flight` is decremented and `drained` is notified,
    // which happens-before set_update_sink(None) returns. If clear had returned
    // early (quiescence contract broken), `callback_done` would be false here.
    assert!(
        callback_done.load(Ordering::SeqCst),
        "set_update_sink(None) returned before on_update completed — quiescence violated"
    );

    clear_handle.join().unwrap();
    helper.join().unwrap();
}

/// Calling `shutdown()` while an `on_update` is in-flight must not UAF or
/// deadlock: `shutdown()` joins the listener thread, which means it must wait
/// for the in-flight callback to return before completing.
///
/// # Proof structure
///
/// `BlockingSink` signals entry via a channel and blocks at a `Barrier`.
/// The passive pre-start snapshot fires `on_update` on registration. After
/// receiving the "entered" signal the callback is genuinely in-flight.
/// A helper thread releases the barrier concurrently — allowing the listener
/// thread (blocked inside `on_update`) to exit, which lets `shutdown()`'s
/// internal thread-join complete cleanly.
///
/// A `recv_timeout` wall-clock deadline ensures a regression (deadlock)
/// surfaces as a test failure rather than a CI hang.
#[test]
fn shutdown_during_in_flight_update_no_uaf_no_deadlock() {
    let app = NmpApp::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Barrier::new(2));

    app.set_update_sink(Some(Box::new(BlockingSink {
        entered_tx: Mutex::new(Some(entered_tx)),
        gate: Arc::clone(&gate),
        callback_done: None,
    })));

    // Wait until on_update is genuinely in-flight — deterministic, no sleep.
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("passive pre-start snapshot never triggered on_update within 2s");

    // Helper releases the gate so the listener thread can eventually exit
    // and shutdown()'s join can complete. Concurrent with shutdown() below.
    let gate_clone = Arc::clone(&gate);
    let helper = thread::spawn(move || gate_clone.wait());

    // Run shutdown() in its own thread so we can apply a wall-clock deadline.
    let app_for_shutdown = Arc::clone(&app);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let shutdown_handle = thread::spawn(move || {
        app_for_shutdown.shutdown();
        let _ = done_tx.send(());
    });

    // If shutdown() deadlocks (e.g., waits on in_flight while holding a lock
    // that the callback needs to decrement), this recv_timeout catches it.
    done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("shutdown() deadlocked while on_update was in-flight — UAF/deadlock regression");

    shutdown_handle.join().unwrap();
    helper.join().unwrap();

    // Idempotent: a second shutdown() is a no-op (Kotlin/Swift contract, #2149).
    app.shutdown();
}

// ── Reentrancy (intentionally untested) ──────────────────────────────────────
//
// Calling any `NmpApp` method from within `on_update` is documented-forbidden
// (see `UpdateSink`). A test that attempted re-entry would deadlock by design:
// the quiescence Condvar (`wait_while`) is not re-entrant, so the re-entrant
// `set_update_sink` call would block forever waiting for `in_flight` to drop
// to zero while the callback itself (holding `in_flight > 0`) is blocked.
// Reentrancy is enforced by contract only — not by a test.

// ── Dispatch wrapper surface ──────────────────────────────────────────────────

/// Malformed envelope (empty bytes) → error outcome, never a panic.
/// Verifies the wrapper correctly surfaces the typed core's D6 guarantee.
#[test]
fn dispatch_malformed_envelope_produces_error_outcome() {
    let app = NmpApp::new();
    let outcome = app.dispatch_action(vec![]);
    assert!(
        outcome.error.is_some(),
        "malformed envelope must produce DispatchOutcome.error"
    );
    assert!(outcome.correlation_id.is_none());
    assert!(outcome.code.is_none());
}

/// Dispatch wrapper passes through `correlation_id` from the typed core.
/// Uses the `nmp.publish` built-in module (registered by default_registry).
#[test]
fn dispatch_wrapper_passes_through_correlation_id() {
    use nmp_core::publish::{PublishAction, PublishTarget};
    use nmp_core::substrate::ActionPayload;

    let app = NmpApp::new();
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: vec![],
        content: "uniffi dispatch test".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    let payload = action.encode();
    let envelope = encode_dispatch_envelope(
        "corr-uniffi-wrap-1",
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let outcome = app.dispatch_action(envelope);
    assert_eq!(
        outcome.correlation_id.as_deref(),
        Some("corr-uniffi-wrap-1"),
        "wrapper must echo the host-supplied correlation_id"
    );
    assert!(outcome.error.is_none());
    assert!(outcome.code.is_none());
}

// ── Action module for coded-rejection wrapper test ───────────────────────────

/// Bytes-capable coded-rejection module for the UniFFI wrapper smoke test.
struct UniffiCodedRejectModule; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]
impl ActionModule for UniffiCodedRejectModule {
    const NAMESPACE: &'static str = "test.uniffi_wrapper.coded_reject"; // doctrine-allow: action_namespace — test fixture
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
        Err(ActionRejection::InvalidCoded {
            code: "uniffi_wrapper_code",
            message: "uniffi wrapper coded rejection".into(),
        })
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

/// Dispatch wrapper passes through `code` for coded rejections (load-bearing:
/// ensures the UniFFI surface doesn't accidentally drop the code field).
#[test]
fn dispatch_wrapper_passes_through_code_field() {
    // NmpApp doesn't expose `register_action` directly on the UniFFI surface
    // (composition APIs come in later slices). Use nmp_native_runtime::new_app
    // + register_action to build the configured inner app, then wrap it.
    let mut inner = nmp_native_runtime::new_app();
    let _ = inner.register_action(UniffiCodedRejectModule);

    // Wrap the pre-configured inner app in the UniFFI NmpApp.
    let app = Arc::new(NmpApp { inner });

    let envelope = encode_dispatch_envelope(
        "corr-code-wrap",
        UniffiCodedRejectModule::NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &[0u8; 4],
    );
    let outcome = app.dispatch_action(envelope);
    assert!(
        outcome.error.as_deref().map_or(false, |e| e.contains("uniffi wrapper coded rejection")),
        "error must carry the human message"
    );
    assert_eq!(
        outcome.code.as_deref(),
        Some("uniffi_wrapper_code"),
        "code must carry the stable machine token through the wrapper"
    );
    assert!(outcome.correlation_id.is_none());
}
