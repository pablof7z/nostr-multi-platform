//! Unit tests for `Session` — factored out of `session.rs` to keep that file
//! under the 500-LOC ceiling (AGENTS.md §File-Size).

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use super::{NextUpdate, Session, insert_session, remove_session, session_arc};

#[test]
fn on_update_forwards_frame_to_mpsc_when_no_generic_sink() {
    // With no generic sink registered, the UniFFI path is a no-op and the
    // frame still reaches the legacy mpsc test seam — never dropped.
    let session = Session::test_session();
    let handle = insert_session(Arc::clone(&session));
    session.callback_state.send(b"frame-bytes".to_vec());
    let update = session.recv_next_update(Duration::from_millis(200));
    assert_eq!(update, NextUpdate::Frame(b"frame-bytes".to_vec()));
    remove_session(handle);
}

#[test]
fn close_updates_wakes_blocked_next_update() {
    let session = Session::test_session();
    let (entered_tx, entered_rx) = mpsc::channel();
    let reader = {
        let session = Arc::clone(&session);
        std::thread::spawn(move || {
            entered_tx.send(()).expect("signal reader entry");
            session.recv_next_update(Duration::from_secs(60))
        })
    };

    entered_rx.recv().expect("reader entered next update");
    session.close_updates();

    assert_eq!(
        reader.join().expect("reader thread joined"),
        NextUpdate::Closed
    );
}

#[test]
fn free_native_is_idempotent_and_does_not_reclaim_blocked_reader_state() {
    let session = Session::test_session();
    let handle = insert_session(Arc::clone(&session));
    let reader_session = session_arc(handle).expect("registry handle exists");
    let (entered_tx, entered_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        entered_tx.send(()).expect("signal reader entry");
        reader_session.recv_next_update(Duration::from_secs(60))
    });

    entered_rx.recv().expect("reader entered next update");
    let removed = remove_session(handle).expect("first remove returns session");
    removed.free_native();
    removed.free_native();

    assert!(session_arc(handle).is_none());
    assert!(remove_session(handle).is_none());
    assert_eq!(
        reader.join().expect("reader thread joined"),
        NextUpdate::Closed
    );
}

// ── FIX 1 — deadlock-prevention test ─────────────────────────────────────────

/// Verify that `close_updates` does NOT deadlock when an in-flight generic-sink
/// callback re-enters `Session::with_app` (which acquires `Session.state`).
///
/// **Root cause of the bug (old code):** `close_updates` held `Session.state`
/// while calling `nmp_app_set_update_callback(None)`, which blocks until any
/// in-flight `on_update` completes.  If that `on_update` called back into
/// `with_app` (which also acquires `state`) the two locks mutually waited →
/// deadlock.
///
/// **Fix:** `close_updates` releases `Session.state` BEFORE the blocking
/// quiescence call (see the 3-phase design in `session.rs`).
///
/// **Caveat:** with an inert (null-app) session the kernel quiescence call
/// (`nmp_app_set_update_callback(None)`) is a no-op, so the live-kernel
/// deadlock path is not reproduced here.  This test validates the structural
/// ordering: a concurrent `close_updates` + callback that calls `with_app`
/// must both complete in finite time (no deadlock), regardless of scheduling.
#[test]
fn close_updates_does_not_deadlock_when_callback_reenters_with_app() {
    use std::sync::{Condvar, Mutex};

    let session = Arc::new(Session::test_session());

    let (started_tx, started_rx) = mpsc::sync_channel::<()>(0);
    let gate: Arc<(Mutex<bool>, Condvar)> =
        Arc::new((Mutex::new(false), Condvar::new()));
    let gate_ref = Arc::clone(&gate);
    let session_ref = Arc::clone(&session);

    // Generic sink: signals entry, waits for the gate, then re-enters with_app.
    // In the old design with close_updates holding state during the quiescence
    // wait, this re-entry would deadlock. In the new design it must not.
    let reentry_fn: Arc<dyn Fn(Vec<u8>) + Send + Sync> = Arc::new(move |_bytes| {
        started_tx.send(()).ok(); // signal: inside callback
        let (lock, cvar) = &*gate_ref;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = cvar.wait(open).unwrap();
        }
        // Re-entry: acquire Session.state (must not deadlock with close_updates).
        let _ = session_ref.with_app(|_| ());
    });
    session.set_generic_sink(reentry_fn);

    // Thread A: fire the generic sink (enters blocking callback).
    let session_a = Arc::clone(&session);
    let sender = std::thread::spawn(move || {
        session_a.callback_state_send_via_generic(b"test".to_vec());
    });

    // Wait for the callback to start.
    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("callback should start within 2 s");

    // Thread B: call close_updates concurrently with the in-flight callback.
    let session_b = Arc::clone(&session);
    let closer = std::thread::spawn(move || {
        session_b.close_updates();
    });

    // Unblock the callback — it will re-enter with_app concurrently with
    // close_updates completing Phase 3 (state lock for cleanup).
    let (lock, cvar) = &*gate;
    *lock.lock().unwrap() = true;
    cvar.notify_all();

    // Both threads must complete without deadlock.
    sender.join().expect("sender thread must not deadlock");
    closer.join().expect("closer thread must not deadlock");
}

// ── FIX 2 — UAF-guard structural test ────────────────────────────────────────

/// Structural test for the `callback_mutation_guard` `RwLock` invariant.
///
/// **Invariant (documented in `session.rs`):** `nmp_app_free(app)` (write lock)
/// cannot run concurrently with any lock-free quiescence/reregister operation
/// (read lock) that is actively using the raw `app` pointer.
///
/// This test verifies the RwLock ordering directly: a thread holding the read
/// lock blocks a concurrent write-lock acquisition until the reader finishes.
/// In production, the read-lock holders are `quiesce_update_callback_lockfree`
/// and `reregister_update_callback_lockfree`; the write-lock holder is the
/// `nmp_app_free` path inside `free_native`.
///
/// The test operates on inert sessions so no actual app allocation is involved;
/// it verifies the guard semantics, not the full `free_native` flow.
#[test]
fn callback_mutation_guard_write_lock_blocks_until_readers_finish() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let session = Arc::new(Session::inert_session());

    let completed = Arc::new(AtomicBool::new(false));
    let completed_ref = Arc::clone(&completed);

    let (read_acquired_tx, read_acquired_rx) = mpsc::sync_channel::<()>(0);
    let (release_read_tx, release_read_rx) = mpsc::channel::<()>();

    // Thread A: hold the read lock (simulates an in-flight lock-free quiescence).
    let session_a = Arc::clone(&session);
    let reader = std::thread::spawn(move || {
        let _read_guard = session_a.callback_mutation_guard.read().unwrap();
        read_acquired_tx.send(()).ok(); // signal: read lock held
        // Hold until signalled — verifies the write lock cannot proceed meanwhile.
        release_read_rx.recv().ok();
        // completed must still be false: writer is blocked by this guard.
        assert!(
            !completed_ref.load(Ordering::SeqCst),
            "write-locked work must not complete while a read lock is held",
        );
        // Drop _read_guard here → writer can proceed.
    });

    read_acquired_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("reader should acquire the lock within 2 s");

    // Thread B: acquire the write lock (simulates the nmp_app_free path).
    // This must block until Thread A releases the read lock.
    let session_b = Arc::clone(&session);
    let completed_ref2 = Arc::clone(&completed);
    let writer = std::thread::spawn(move || {
        let _write_guard = session_b.callback_mutation_guard.write().unwrap();
        completed_ref2.store(true, Ordering::SeqCst);
    });

    // Release the read lock — this unblocks the writer.
    release_read_tx.send(()).ok();
    reader.join().expect("reader must complete");
    writer.join().expect("writer must complete");

    assert!(
        completed.load(Ordering::SeqCst),
        "write-locked work must complete after the read lock is released",
    );
}

/// Verify that concurrent `free_native` + `quiesce_update_callback_lockfree`
/// on an inert session complete without panic or assertion failure.
///
/// This exercises the guard-path code on a null-app session where neither
/// `nmp_app_set_update_callback` nor `nmp_app_free` is actually called, but
/// the guard acquisition/release sequence runs end-to-end.
#[test]
fn free_native_concurrent_with_quiesce_lockfree_on_inert_session() {
    let session = Arc::new(Session::inert_session());

    let session_a = Arc::clone(&session);
    let session_b = Arc::clone(&session);

    let a = std::thread::spawn(move || {
        session_a.quiesce_update_callback_lockfree();
    });
    let b = std::thread::spawn(move || {
        session_b.free_native();
    });

    a.join().expect("quiesce thread must not panic");
    b.join().expect("free_native thread must not panic");
}
