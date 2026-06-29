//! Drain-gate quiescence tests for the action-result observer (M14-C-tail /
//! #2429).
//!
//! Deterministic, channel-synchronised, NO sleeps in the logic path — the only
//! timeout is the bounded *negative* check that the setter does NOT return while
//! a delivery is mid-flight (the proven `capability_socket_quiescence_tests.rs`
//! idiom). A drain regression makes the setter return early, flipping that
//! `recv_timeout(...).is_err()` to a failing assertion.

use super::ActionRegistry;
use crate::substrate::ActionResult;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn result(id: &str) -> ActionResult {
    ActionResult {
        correlation_id: id.to_string(),
        result_json: serde_json::Value::Null,
    }
}

/// Install a blocking observer that signals entry, blocks on `release_rx`, then
/// increments `completed` AFTER release.
fn install_blocking_observer(
    registry: &ActionRegistry,
    started_tx: mpsc::SyncSender<()>,
    release_rx: mpsc::Receiver<()>,
    completed: Arc<AtomicU32>,
) {
    let started_tx = Mutex::new(Some(started_tx));
    let release_rx = Mutex::new(release_rx);
    registry.set_result_observer(move |_r| {
        if let Ok(mut g) = started_tx.lock() {
            let _ = g.take().map(|tx| tx.send(()));
        }
        let _ = release_rx.lock().unwrap().recv();
        completed.fetch_add(1, Ordering::SeqCst);
    });
}

/// Register → clear → clear: idempotent, never deadlocks, ends unregistered,
/// and delivery after clear is a silent no-op.
#[test]
fn result_observer_register_clear_idempotent() {
    let registry = ActionRegistry::new();
    let hits = Arc::new(AtomicU32::new(0));
    let h = Arc::clone(&hits);
    registry.set_result_observer(move |_r| {
        h.fetch_add(1, Ordering::SeqCst);
    });
    assert!(registry.has_result_observer());
    registry.clear_result_observer();
    assert!(!registry.has_result_observer());
    registry.clear_result_observer(); // idempotent
    assert!(!registry.has_result_observer());

    registry.deliver_result(result("after-clear"));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "cleared observer must not fire"
    );
}

/// THE drain proof: `clear_result_observer()` must block while a delivery is
/// in-flight and return only after it completes.
#[test]
fn clear_waits_for_in_flight_delivery() {
    let registry = Arc::new(ActionRegistry::new());
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicU32::new(0));

    install_blocking_observer(&registry, started_tx, release_rx, Arc::clone(&completed));

    let reg_for_deliver = Arc::clone(&registry);
    let deliver = thread::spawn(move || {
        reg_for_deliver.deliver_result(result("in-flight"));
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("delivery should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let reg_for_clear = Arc::clone(&registry);
    let setter = thread::spawn(move || {
        reg_for_clear.clear_result_observer();
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "clear_result_observer() returned while a delivery was still mid-flight — quiescence violated"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("clear_result_observer() should return after the delivery drains");

    setter.join().unwrap();
    deliver.join().unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    assert!(!registry.has_result_observer());
}

/// Replacing the observer while a delivery is in-flight also drains.
#[test]
fn replace_waits_for_in_flight_delivery() {
    let registry = Arc::new(ActionRegistry::new());
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicU32::new(0));

    install_blocking_observer(&registry, started_tx, release_rx, Arc::clone(&completed));

    let reg_for_deliver = Arc::clone(&registry);
    let deliver = thread::spawn(move || {
        reg_for_deliver.deliver_result(result("in-flight-2"));
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("delivery should start");

    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel(1);
    let reg_for_replace = Arc::clone(&registry);
    let setter = thread::spawn(move || {
        reg_for_replace.set_result_observer(|_r| {});
        setter_done_tx.send(()).unwrap();
    });

    assert!(
        setter_done_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "set_result_observer() returned while a delivery was still mid-flight — quiescence violated"
    );
    release_tx.send(()).unwrap();
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("set_result_observer() should return after the delivery drains");

    setter.join().unwrap();
    deliver.join().unwrap();
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    registry.clear_result_observer();
}
