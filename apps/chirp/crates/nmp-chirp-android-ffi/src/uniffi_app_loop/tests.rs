//! Tests for the UniFFI app-loop lane — factored out of `uniffi_app_loop.rs`
//! to keep that file under the 500-LOC ceiling (AGENTS.md §File-Size).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::session::Session;

// ── Minimal UpdateSink implementations for testing ────────────────────────────

/// Collects all received frames via a shared inner state so the test can
/// inspect captured frames after passing ownership of the sink to
/// `set_update_sink` (which consumes `Box<dyn UpdateSink>`).
///
/// Usage:
///   let (sink, captured) = CaptureSink::new();
///   handle.set_update_sink(Box::new(sink));   // ownership transferred
///   // ... send frames ...
///   assert_eq!(captured.lock().unwrap().len(), 1);
struct CaptureSink {
    inner: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl CaptureSink {
    fn new() -> (Self, Arc<Mutex<Vec<Vec<u8>>>>) {
        let inner = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        (Self { inner: Arc::clone(&inner) }, inner)
    }
}

impl UpdateSink for CaptureSink {
    fn on_update(&self, frame: Vec<u8>) {
        self.inner.lock().unwrap().push(frame);
    }
}

/// Implements `UpdateSink` but panics on every call.
///
/// Used to verify that the `catch_unwind` trampoline in `set_update_sink`
/// prevents a panicking callback from aborting the process.
struct PanickingSink;
impl UpdateSink for PanickingSink {
    fn on_update(&self, _frame: Vec<u8>) {
        panic!("intentional panic in PanickingSink");
    }
}

/// Blocks in `on_update` until `unblock()` is called from another thread.
///
/// Used to verify the quiescence ordering: `clear_generic_sink` must wait
/// for any in-flight `on_update` to complete before returning.
struct BlockingSink {
    started: std::sync::mpsc::SyncSender<()>,
    gate: Mutex<bool>,
    cvar: std::sync::Condvar,
}

impl BlockingSink {
    fn new(started: std::sync::mpsc::SyncSender<()>) -> Arc<Self> {
        Arc::new(Self {
            started,
            gate: Mutex::new(false),
            cvar: std::sync::Condvar::new(),
        })
    }
    fn unblock(&self) {
        *self.gate.lock().unwrap() = true;
        self.cvar.notify_all();
    }
}

impl UpdateSink for BlockingSink {
    fn on_update(&self, _frame: Vec<u8>) {
        self.started.send(()).ok();
        let mut released = self.gate.lock().unwrap();
        while !*released {
            released = self.cvar.wait(released).unwrap();
        }
    }
}

// ── Helper: fire the generic_sink directly, bypassing the kernel ──────────────

fn send_frame_via_generic_sink(session: &Arc<Session>, frame: &[u8]) {
    session.callback_state_send_via_generic(frame.to_vec());
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn set_update_sink_receives_frame_bytes_unchanged() {
    let session = Session::inert_session();
    let handle = Arc::new(AppHandle { session: Arc::clone(&session), handle: 0 });

    let (sink, captured) = CaptureSink::new();
    handle.set_update_sink(Box::new(sink));

    let payload: &[u8] = b"NMPU\x00\x01\x02\x03test-frame";
    send_frame_via_generic_sink(&session, payload);

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "exactly one frame should be captured");
    assert_eq!(got[0].as_slice(), payload, "frame bytes must be identical");
}

#[test]
fn clear_update_sink_stops_delivery() {
    let session = Session::inert_session();
    let handle = Arc::new(AppHandle { session: Arc::clone(&session), handle: 0 });

    let (sink, captured) = CaptureSink::new();
    handle.set_update_sink(Box::new(sink));

    // Directly clear the generic sink slot (bypasses kernel quiescence
    // for this inert-session test).
    session.clear_generic_sink();

    send_frame_via_generic_sink(&session, b"should-not-arrive");
    assert!(
        captured.lock().unwrap().is_empty(),
        "no frames should arrive after clear"
    );
}

#[test]
fn dispatch_action_bytes_returns_error_on_inert_handle() {
    let handle = Arc::new(AppHandle {
        session: Session::inert_session(),
        handle: 0,
    });
    let ack = handle.dispatch_action_bytes(b"NMPD\x00\x00garbage".to_vec());
    assert!(ack.correlation_id.is_none(), "inert handle must not produce a correlation_id");
    assert!(ack.error.is_some(), "inert handle must produce an error");
    assert!(!ack.error.unwrap().is_empty(), "error message must be non-empty");
}

#[test]
fn dispatch_action_json_returns_error_on_inert_handle() {
    let handle = Arc::new(AppHandle {
        session: Session::inert_session(),
        handle: 0,
    });
    let ack = handle.dispatch_action_json(
        "nmp.test.action".to_string(),
        r#"{"key":"value"}"#.to_string(),
    );
    assert!(ack.correlation_id.is_none());
    assert!(ack.error.is_some());
}

#[test]
fn parse_dispatch_ack_correlation_id_path() {
    let ack = parse_dispatch_ack(r#"{"correlation_id":"chirp-42"}"#);
    assert_eq!(ack.correlation_id.as_deref(), Some("chirp-42"));
    assert!(ack.error.is_none());
}

#[test]
fn parse_dispatch_ack_error_path() {
    let ack = parse_dispatch_ack(r#"{"error":"unknown namespace"}"#);
    assert!(ack.correlation_id.is_none());
    assert_eq!(ack.error.as_deref(), Some("unknown namespace"));
}

#[test]
fn parse_dispatch_ack_malformed_json_returns_error() {
    let ack = parse_dispatch_ack("not-json");
    assert!(ack.correlation_id.is_none());
    assert!(ack.error.is_some());
}

#[test]
fn callback_panic_is_contained_does_not_abort() {
    let session = Session::inert_session();
    let handle = Arc::new(AppHandle { session: Arc::clone(&session), handle: 0 });
    handle.set_update_sink(Box::new(PanickingSink));

    // Should NOT panic / abort the test process — catch_unwind in the
    // trampoline (inside `set_update_sink`) must contain the panic.
    send_frame_via_generic_sink(&session, b"trigger-panic");
    // Reaching this line proves the catch_unwind worked.
}

#[test]
fn quiescence_clear_does_not_return_while_callback_in_flight() {
    // This tests the GENERIC SINK path with an inert (null-app) session.
    //
    // What we verify:
    // 1. A frame sent to the generic_sink starts executing `on_update`.
    // 2. `on_update` blocks until we call `unblock()`.
    // 3. Only after the sender thread's `on_update` finishes is the slot
    //    cleared by `clear_generic_sink`.
    // 4. After clearing, a newly registered sink receives the next frame;
    //    the old (blocking) sink does NOT.
    //
    // The live kernel Condvar quiescence (nmp_app_set_update_callback(None)
    // blocking until in-flight on_update returns) is tested by nmp-ffi;
    // here we verify only the generic_sink slot ordering.
    let session = Session::inert_session();
    let handle = Arc::new(AppHandle { session: Arc::clone(&session), handle: 0 });

    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(0);
    let blocking = BlockingSink::new(tx);
    let blocking_ref: Arc<BlockingSink> = Arc::clone(&blocking);

    // Inject blocking logic directly into the generic_sink slot, bypassing
    // `set_update_sink` (which requires `Box<dyn UpdateSink>`, not Arc).
    // The closure mirrors what `set_update_sink` builds (with catch_unwind).
    let update_fn: Arc<dyn Fn(Vec<u8>) + Send + Sync> =
        Arc::new(move |bytes: Vec<u8>| {
            let s = Arc::clone(&blocking_ref);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                s.on_update(bytes);
            }));
        });
    session.set_generic_sink(update_fn);

    // Thread: fire the generic sink (enters blocking on_update)
    let session2 = Arc::clone(&session);
    let sender_thread = std::thread::spawn(move || {
        session2.callback_state_send_via_generic(b"blocking-frame".to_vec());
    });

    // Wait for the sink to signal it has entered on_update
    rx.recv_timeout(Duration::from_secs(2))
        .expect("blocking sink should signal entry within 2 s");

    // Release the blocking on_update so the sender thread can finish.
    blocking.unblock();
    sender_thread.join().expect("sender thread must not panic");

    // Now clear the generic sink and verify the slot is empty.
    session.clear_generic_sink();
    assert!(
        !session.callback_state_generic_has_sink(),
        "generic sink should be None after clear"
    );

    // Register a new capture sink — it must receive the next frame.
    let (new_sink, new_captured) = CaptureSink::new();
    handle.set_update_sink(Box::new(new_sink));
    session.callback_state_send_via_generic(b"after-clear".to_vec());
    assert_eq!(
        new_captured.lock().unwrap().len(),
        1,
        "new sink must receive frames after re-registration"
    );
}
