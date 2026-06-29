//! Quiescence / drain-gate tests for [`LifecycleObserverGate`] (M14-C-tail /
//! #2429).
//!
//! Deterministic, channel-synchronised, NO sleeps in the logic path — the only
//! timeout is the bounded *negative* check that the setter does NOT return while
//! the callback is still mid-flight (the same proven idiom as
//! `capability_socket_quiescence_tests.rs`). A drain-ordering regression makes
//! the setter return early, flipping that `recv_timeout(...).is_err()` to a
//! failing assertion rather than hanging.
//!
//! The native observer path is exercised because it is the path the UniFFI
//! `LifecycleSink` uses and the one whose ARC must not be released while the
//! actor is mid-call.

use super::{
    handle_lifecycle_event, new_observer_slot, LifecycleObserverGate, NativeLifecycleObserver,
};
use crate::kernel::{Kernel, LifecyclePhase};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Native observer that signals entry via `started_tx`, blocks on `release_rx`
/// (controlled by the test's main thread), then increments `completed` AFTER
/// release. A drain regression lets the setter return before `completed` is
/// bumped.
fn blocking_observer(
    started_tx: mpsc::SyncSender<()>,
    release_rx: mpsc::Receiver<()>,
    completed: Arc<AtomicU32>,
) -> NativeLifecycleObserver {
    let started_tx = Mutex::new(Some(started_tx));
    let release_rx = Mutex::new(release_rx);
    Arc::new(move |_phase: u32| {
        if let Ok(mut g) = started_tx.lock() {
            let _ = g.take().map(|tx| tx.send(()));
        }
        let _ = release_rx.lock().unwrap().recv();
        completed.fetch_add(1, Ordering::SeqCst);
    })
}

fn fire_in_background(slot: &Arc<LifecycleObserverGate>) -> thread::JoinHandle<()> {
    let slot = Arc::clone(slot);
    thread::spawn(move || {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
    })
}

/// Register → clear → clear: idempotent, never deadlocks, ends unregistered.
#[test]
fn native_observer_register_clear_idempotent() {
    let slot = new_observer_slot();
    let calls = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&calls);
    slot.set_native_observer(Some(Arc::new(move |_p| {
        c.fetch_add(1, Ordering::SeqCst);
    })));
    assert!(slot.is_registered());
    slot.clear();
    assert!(!slot.is_registered());
    slot.clear(); // idempotent second clear
    assert!(!slot.is_registered());
}

/// Replace native-with-native (last-writer-wins): only the latest observer is
/// live.
#[test]
fn native_observer_replace_routes_to_latest() {
    let slot = new_observer_slot();
    let a = Arc::new(AtomicU32::new(0));
    let b = Arc::new(AtomicU32::new(0));
    let (a2, b2) = (Arc::clone(&a), Arc::clone(&b));
    slot.set_native_observer(Some(Arc::new(move |_p| {
        a2.fetch_add(1, Ordering::SeqCst);
    })));
    slot.set_native_observer(Some(Arc::new(move |_p| {
        b2.fetch_add(1, Ordering::SeqCst);
    })));

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
    assert_eq!(a.load(Ordering::SeqCst), 0, "replaced observer must not fire");
    assert_eq!(b.load(Ordering::SeqCst), 1, "latest observer fires");
    slot.clear();
}

/// THE drain proof: `clear()` must block while a native observer is in-flight
/// and return only after it completes.
#[test]
fn clear_waits_for_in_flight_native_observer() {
    let slot = new_observer_slot();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicU32::new(0));

    slot.set_native_observer(Some(blocking_observer(
        started_tx,
        release_rx,
        Arc::clone(&completed),
    )));

    let event = fire_in_background(&slot);
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("observer should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let setter_slot = Arc::clone(&slot);
    let setter = thread::spawn(move || {
        setter_slot.clear();
        setter_done_tx.send(()).unwrap();
    });

    // Negative check: clear() must NOT return while the observer is mid-flight.
    // A drain regression flips this to a failing assertion.
    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "clear() returned while the lifecycle observer was still mid-flight — quiescence violated"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear() should return after the observer drains");

    setter.join().unwrap();
    event.join().unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    assert!(!slot.is_registered(), "slot must be empty after clear()");
}

/// Replacing an in-flight observer also drains: the previous observer is neither
/// registered nor mid-invocation when the setter returns.
#[test]
fn replace_waits_for_in_flight_native_observer() {
    let slot = new_observer_slot();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicU32::new(0));

    slot.set_native_observer(Some(blocking_observer(
        started_tx,
        release_rx,
        Arc::clone(&completed),
    )));

    let event = fire_in_background(&slot);
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("observer should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let setter_slot = Arc::clone(&slot);
    let setter = thread::spawn(move || {
        setter_slot.set_native_observer(Some(Arc::new(|_p| {})));
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "replace returned while the previous observer was still mid-flight — quiescence violated"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("replace should return after the previous observer drains");

    setter.join().unwrap();
    event.join().unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    slot.clear();
}
