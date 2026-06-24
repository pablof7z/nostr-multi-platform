//! Blocking regression tests for capability-callback quiescence.

use super::{CapabilityCallbackRegistration, dispatch_capability, new_capability_callback_slot};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

extern "C" fn replacement_handler(ctx: *mut c_void, req: *const c_char) -> *mut c_char {
    let calls = unsafe { &*(ctx as *const AtomicU32) };
    calls.fetch_add(1, Ordering::SeqCst);
    let s = unsafe { CStr::from_ptr(req) }
        .to_string_lossy()
        .into_owned();
    CString::new(s).unwrap().into_raw()
}

#[test]
fn unregister_waits_for_in_flight_callback_to_finish() {
    let slot = new_capability_callback_slot();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let ctx = Box::new(BlockingHandlerContext {
        started_tx,
        release_rx: Mutex::new(release_rx),
        completed: AtomicU32::new(0),
    });
    slot.set_registration(Some(CapabilityCallbackRegistration {
        context: (&*ctx as *const BlockingHandlerContext) as usize,
        callback: blocking_handler,
    }));

    let dispatch_slot = Arc::clone(&slot);
    let dispatch = thread::spawn(move || {
        dispatch_capability(
            &dispatch_slot,
            r#"{"namespace":"test","correlation_id":"c4"}"#,
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("callback should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let setter_slot = Arc::clone(&slot);
    let setter = thread::spawn(move || {
        setter_slot.clear();
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "unregister returned while the capability callback was still mid-flight"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("unregister should return after callback drains");
    setter.join().unwrap();
    let out = dispatch.join().unwrap();
    assert_eq!(out, r#"{"namespace":"test","correlation_id":"c4"}"#);
    assert_eq!(ctx.completed.load(Ordering::SeqCst), 1);

    let no_handler = dispatch_capability(&slot, r#"{"namespace":"test","correlation_id":"c5"}"#);
    assert!(no_handler.contains("no-capability-handler"));
}

#[test]
fn replace_waits_for_in_flight_old_callback_and_installs_new() {
    let slot = new_capability_callback_slot();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let old_ctx = Box::new(BlockingHandlerContext {
        started_tx,
        release_rx: Mutex::new(release_rx),
        completed: AtomicU32::new(0),
    });
    let new_calls = Arc::new(AtomicU32::new(0));
    slot.set_registration(Some(CapabilityCallbackRegistration {
        context: (&*old_ctx as *const BlockingHandlerContext) as usize,
        callback: blocking_handler,
    }));

    let dispatch_slot = Arc::clone(&slot);
    let dispatch = thread::spawn(move || {
        dispatch_capability(
            &dispatch_slot,
            r#"{"namespace":"old","correlation_id":"c6"}"#,
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("old callback should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let setter_slot = Arc::clone(&slot);
    let new_calls_for_setter = Arc::clone(&new_calls);
    let setter = thread::spawn(move || {
        setter_slot.set_registration(Some(CapabilityCallbackRegistration {
            context: Arc::as_ptr(&new_calls_for_setter) as usize,
            callback: replacement_handler,
        }));
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "replace returned while the old capability callback was still mid-flight"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("replace should return after old callback drains");
    setter.join().unwrap();
    assert_eq!(old_ctx.completed.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatch.join().unwrap(),
        r#"{"namespace":"old","correlation_id":"c6"}"#
    );

    let out = dispatch_capability(&slot, r#"{"namespace":"new","correlation_id":"c7"}"#);
    assert_eq!(out, r#"{"namespace":"new","correlation_id":"c7"}"#);
    assert_eq!(
        new_calls.load(Ordering::SeqCst),
        1,
        "new dispatch should use the replacement context"
    );
}
