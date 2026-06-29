//! Regression tests for issue #618: `nmp_app_new` is passive and still gives
//! snapshot-first hosts a pre-start frame through the update callback.

use super::{
    UpdateCallback, app_ref, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start,
};
use nmp_core::decode_snapshot_envelope;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

static UPDATE_TX: OnceLock<Mutex<Option<Sender<Vec<u8>>>>> = OnceLock::new();
static SERIAL: Mutex<()> = Mutex::new(());

extern "C" fn capture_update(_ctx: *mut c_void, bytes: *const u8, len: usize) {
    if bytes.is_null() {
        return;
    }
    // SAFETY: the FFI update callback receives a frame pointer that is valid
    // for the duration of the call; the test copies it before returning.
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(frame);
            }
        }
    }
}

fn install_capture() -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = channel::<Vec<u8>>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_capture() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

#[test]
fn passive_handle_delivers_prestart_snapshot_on_callback_registration() {
    let _guard = SERIAL.lock().unwrap();
    let rx = install_capture();
    let app = nmp_app_new();

    let app_ref = app_ref(app).expect("app");
    assert!(!app_ref.is_alive(), "new handle must be passive");
    let clock = Arc::new(nmp_core::MonotonicSecondClock::new(
        UNIX_EPOCH + Duration::from_millis(1_700_000_123_456),
    ));
    app_ref.set_kernel_clock_for_test(Arc::clone(&clock));
    app_ref.set_queue_depth_for_test(7);

    nmp_app_set_update_callback(
        app,
        std::ptr::null_mut(),
        Some(capture_update as UpdateCallback),
    );

    let frame = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("passive pre-start frame delivered");
    let envelope = decode_snapshot_envelope(&frame).expect("pre-start frame is a snapshot");
    assert!(
        !envelope.running,
        "passive pre-start frame must be running=false"
    );
    assert_eq!(
        envelope.last_tick_ms, 1_700_000_123_456,
        "passive pre-start frame must use the injected kernel clock"
    );
    assert_eq!(
        envelope.actor_queue_depth, 7,
        "passive pre-start frame must read the kernel-bound queue-depth handle"
    );
    app_ref.set_queue_depth_for_test(0);

    nmp_app_start(app, 256, 4);
    assert!(app_ref.is_alive(), "start spawns the actor");
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);
    uninstall_capture();
}
