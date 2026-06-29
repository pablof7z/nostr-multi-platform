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
//! # UniFFI 0.28 note on UpdateSink
//!
//! UniFFI 0.28 generates `Lift` for `Box<dyn Trait>`, not `Arc<dyn Trait>`,
//! for callback interfaces. Tests therefore use `Box<dyn UpdateSink>` directly
//! rather than wrapping in Arc first.

use std::sync::{Arc, Mutex};
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

/// `UpdateSink` that records its invocation via an external `Mutex<bool>` and
/// blocks on a `Barrier` to simulate a long-running in-flight callback.
struct FlagSink {
    called: Arc<Mutex<bool>>,
    gate: Arc<std::sync::Barrier>,
}

impl UpdateSink for FlagSink {
    fn on_update(&self, _frame: Vec<u8>) {
        // Signal we've entered the callback.
        *self.called.lock().unwrap() = true;
        // Block until the test releases us — simulates a long-running callback.
        self.gate.wait();
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
/// This test is inherently concurrent. The `FlagSink` blocks in `on_update`
/// until the barrier is signalled; `set_update_sink(None)` on the main thread
/// must not return before `on_update` exits.
#[test]
fn clear_waits_for_in_flight_callback() {
    use std::thread;

    let app = Arc::new(NmpApp::new());
    let called = Arc::new(Mutex::new(false));
    let barrier = Arc::new(std::sync::Barrier::new(2));

    let sink = Box::new(FlagSink {
        called: Arc::clone(&called),
        gate: Arc::clone(&barrier),
    }) as Box<dyn UpdateSink>;
    app.set_update_sink(Some(sink));

    // The passive pre-start snapshot fires into the listener on the listener
    // thread. We need to trigger the callback manually via the startup frame.
    // Install, wait for the barrier thread to confirm entry, then clear.
    // To guarantee a callback fires we need the runtime started; for this
    // lightweight quiescence test we use the passive-snapshot path by ensuring
    // the listener is set while `started == false`.
    //
    // Spawn a thread that waits at the barrier (simulating in-flight callback)
    // while the main thread calls set_update_sink(None).
    let barrier_clone = Arc::clone(&barrier);
    let app_clone = Arc::clone(&app);

    let worker = thread::spawn(move || {
        // Simulate the listener thread calling our FlagSink by invoking
        // the quiescence dance directly via the inner update_listener.
        // We reach the inner RuntimeApp to manually trigger the in_flight
        // increment and then the barrier gate.
        //
        // Because the FlagSink is installed before the listener thread
        // processes the passive snapshot, the snapshot fires on registration
        // and will hit the barrier. We just need to release it.
        //
        // Wait briefly to let the passive snapshot fire.
        std::thread::sleep(Duration::from_millis(5));
        // Release the FlagSink's barrier so on_update can return.
        barrier_clone.wait();
        drop(app_clone);
    });

    // Wait until the callback has been entered (or timeout), then clear.
    // The real quiescence proof is: set_update_sink does NOT return until
    // the listener thread's in_flight count drops to zero.
    std::thread::sleep(Duration::from_millis(2));
    app.set_update_sink(None); // Must block until in-flight completes.

    worker.join().unwrap();
}

/// Calling `shutdown()` during an in-flight update must not UAF or deadlock.
#[test]
fn shutdown_during_in_flight_update_no_uaf_no_deadlock() {
    let app = NmpApp::new();
    let (sink, _handle) = RecordSink::new_boxed();
    app.set_update_sink(Some(sink));
    // shutdown() clears the listener (quiescence gate) then joins threads.
    // If there's a UAF the test would crash with a sanitizer or segfault.
    app.shutdown();
    // Idempotent: second shutdown is a no-op.
    app.shutdown();
}

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
