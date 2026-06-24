//! Contract tests for capability-callback quiescence at the exported FFI seam.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nmp_core::__ffi_internal::{CapabilityCallbackSlot, dispatch_capability};

use super::{NmpApp, nmp_app_free, nmp_app_new, nmp_app_set_capability_callback};

struct BlockingHandlerContext {
    started_tx: mpsc::SyncSender<()>,
    release_rx: Mutex<mpsc::Receiver<()>>,
    completed: AtomicU32,
}

extern "C" fn blocking_handler(ctx: *mut c_void, req: *const c_char) -> *mut c_char {
    let ctx = unsafe { &*(ctx as *const BlockingHandlerContext) };
    ctx.started_tx.send(()).unwrap();
    ctx.release_rx.lock().unwrap().recv().unwrap();
    ctx.completed.fetch_add(1, Ordering::SeqCst);
    let s = unsafe { CStr::from_ptr(req) }
        .to_string_lossy()
        .into_owned();
    CString::new(s).unwrap().into_raw()
}

fn install_blocking_handler(
    app: *mut NmpApp,
) -> (
    Box<BlockingHandlerContext>,
    mpsc::Receiver<()>,
    mpsc::SyncSender<()>,
) {
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let ctx = Box::new(BlockingHandlerContext {
        started_tx,
        release_rx: Mutex::new(release_rx),
        completed: AtomicU32::new(0),
    });
    nmp_app_set_capability_callback(
        app,
        (&*ctx as *const BlockingHandlerContext) as *mut c_void,
        Some(blocking_handler),
    );
    (ctx, started_rx, release_tx)
}

fn app_capability_slot(app: *mut NmpApp) -> CapabilityCallbackSlot {
    unsafe { Arc::clone(&(*app).capability_callback) }
}

#[test]
fn ffi_unregister_waits_for_in_flight_capability_callback() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let slot = app_capability_slot(app);
    let (ctx, started_rx, release_tx) = install_blocking_handler(app);

    let dispatch_slot = Arc::clone(&slot);
    let dispatch = thread::spawn(move || {
        dispatch_capability(
            &dispatch_slot,
            r#"{"namespace":"ffi","correlation_id":"c1"}"#,
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("capability callback should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let app_addr = app as usize;
    let setter = thread::spawn(move || {
        nmp_app_set_capability_callback(app_addr as *mut NmpApp, std::ptr::null_mut(), None);
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "FFI unregister returned while the capability callback was still mid-flight"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("FFI unregister should return after callback drains");
    setter.join().unwrap();
    assert_eq!(ctx.completed.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatch.join().unwrap(),
        r#"{"namespace":"ffi","correlation_id":"c1"}"#
    );

    nmp_app_free(app);
}

#[test]
fn app_free_waits_for_actor_owned_capability_slot_to_quiesce() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let slot = app_capability_slot(app);
    let (ctx, started_rx, release_tx) = install_blocking_handler(app);

    let dispatch_slot = Arc::clone(&slot);
    let dispatch = thread::spawn(move || {
        dispatch_capability(
            &dispatch_slot,
            r#"{"namespace":"ffi","correlation_id":"c2"}"#,
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("capability callback should start");

    let (free_done_tx, free_done_rx) = mpsc::sync_channel(1);
    let app_addr = app as usize;
    let free_thread = thread::spawn(move || {
        nmp_app_free(app_addr as *mut NmpApp);
        free_done_tx.send(()).unwrap();
    });

    assert!(
        free_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "nmp_app_free returned while the capability callback was still mid-flight"
    );
    release_tx.send(()).unwrap();
    free_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("nmp_app_free should return after callback drains");
    free_thread.join().unwrap();
    assert_eq!(ctx.completed.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatch.join().unwrap(),
        r#"{"namespace":"ffi","correlation_id":"c2"}"#
    );
}
