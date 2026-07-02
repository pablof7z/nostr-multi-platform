//! Blocking regression tests for capability-callback quiescence.

use super::{dispatch_capability, new_capability_callback_slot};
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

fn blocking_handler(ctx: Arc<BlockingHandlerContext>) -> impl Fn(String) -> String {
    move |req: String| {
        ctx.started_tx.send(()).unwrap();
        ctx.release_rx.lock().unwrap().recv().unwrap();
        ctx.completed.fetch_add(1, Ordering::SeqCst);
        req
    }
}

fn replacement_handler(calls: Arc<AtomicU32>) -> impl Fn(String) -> String {
    move |req: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        req
    }
}

#[test]
fn unregister_waits_for_in_flight_callback_to_finish() {
    let slot = new_capability_callback_slot();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let ctx = Arc::new(BlockingHandlerContext {
        started_tx,
        release_rx: Mutex::new(release_rx),
        completed: AtomicU32::new(0),
    });
    slot.set_native_handler(Some(Arc::new(blocking_handler(Arc::clone(&ctx)))));

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
    let old_ctx = Arc::new(BlockingHandlerContext {
        started_tx,
        release_rx: Mutex::new(release_rx),
        completed: AtomicU32::new(0),
    });
    let new_calls = Arc::new(AtomicU32::new(0));
    slot.set_native_handler(Some(Arc::new(blocking_handler(Arc::clone(&old_ctx)))));

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
        setter_slot.set_native_handler(Some(Arc::new(replacement_handler(new_calls_for_setter))));
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
