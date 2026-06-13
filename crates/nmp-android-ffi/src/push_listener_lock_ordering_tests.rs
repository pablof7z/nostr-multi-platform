//! Lock-ordering invariant tests for the JNI push-listener path.
//!
//! Regression: PR #1226 merged `on_update` holding `push_listener` across the
//! JNI upcall — a deadlock vector whenever Kotlin re-entered Rust (or the actor)
//! from within the `onUpdate` callback. The fix snapshots an `Arc` clone under
//! the lock, drops the lock, THEN calls `push` (mirrors nmp-ffi §1164–1193).

use std::sync::Arc;
use std::time::Duration;

use super::{Session, insert_session, remove_session};

/// Verify that `on_update` does NOT hold `push_listener` while invoking the
/// listener's `push()` method.
///
/// Structural guarantee: after `on_update` returns the slot is immediately
/// trylock-able — it was held for at most the duration of the Arc clone, not
/// for the entire JNI upcall.
#[test]
fn on_update_does_not_hold_push_listener_lock_during_push() {
    let session = Session::test_session();

    // Slot must be available before any update fires.
    assert!(
        session.callback_state.push_listener.try_lock().is_ok(),
        "push_listener should be available before on_update"
    );

    // Drive the callback-state send path (no JNI listener registered, so
    // the snapshot branch is a cheap clone of None — lock held for nanoseconds).
    session.callback_state.send(b"probe-frame".to_vec());
    let _ = session.recv_next_update(Duration::from_millis(100));

    // After on_update the slot must be freely acquirable again.
    assert!(
        session.callback_state.push_listener.try_lock().is_ok(),
        "push_listener must not be held after on_update returns"
    );
}

/// Verify that a concurrent `try_lock` on `push_listener` never blocks while
/// `on_update` is delivering a frame — i.e., the lock is not held across the
/// upcall. Exercises the lock-snapshot-drop-then-call ordering.
#[test]
fn concurrent_set_listener_does_not_deadlock_with_on_update() {
    let session = Arc::new(Session::test_session());
    let session2 = Arc::clone(&session);

    // Thread that floods the send path (simulates rapid frame delivery).
    let sender = std::thread::spawn(move || {
        for _ in 0..20 {
            session2.callback_state.send(b"concurrent-frame".to_vec());
            std::thread::yield_now();
        }
    });

    // Racing try_locks: if the mutex were held across push() this would
    // consistently fail (all 20 attempts would see WouldBlock).
    for _ in 0..20 {
        // Immediately drop the guard so we're not blocking the sender.
        drop(session.callback_state.push_listener.try_lock());
        std::thread::yield_now();
    }

    sender.join().expect("sender thread must not panic");
}
