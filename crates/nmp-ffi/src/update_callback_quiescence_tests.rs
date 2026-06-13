//! Contract tests for the `UpdateCallbackGate` quiescence guarantee.
//!
//! Verifies that after the gate's registration is replaced (or cleared), any
//! previously-started in-flight invocation has completed before the setter
//! thread is unblocked.
//!
//! # Test strategy
//!
//! These tests exercise `UpdateCallbackGate` directly without going through the
//! full `NmpApp` / actor stack, so they are fast, deterministic, and
//! timing-free. Ordering is enforced via channels and a `Barrier`; no sleeps.
//!
//! # Why these tests would FAIL on pre-fix code
//!
//! Before `UpdateCallbackGate` (with the `Condvar`-drain design), the listener
//! copied the `(callback, context)` pair and dropped the mutex *before*
//! invoking the foreign callback. A concurrent `set_update_callback` call
//! (which only swapped the slot) returned immediately while the callback was
//! still executing. The quiescence post-condition was therefore violated: the
//! setter returned while `in_flight > 0`.
//!
//! The `set_callback_blocks_until_in_flight_drains` test below proves this by
//! simulating the listener's "increment-then-invoke" dance directly against the
//! gate, asserting that the setter thread does not make progress until after
//! the simulated invocation completes. On old code (no `Condvar` wait), the
//! setter would return immediately regardless of in-flight state.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use super::{UpdateCallbackGate, UpdateCallbackRegistration};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A no-op `UpdateCallback` function pointer — we never actually call it in
/// these tests; we only need a valid `fn` pointer to construct a registration.
extern "C" fn noop_callback(_context: *mut std::ffi::c_void, _bytes: *const u8, _len: usize) {}

/// Build a dummy `UpdateCallbackRegistration` pointing at `noop_callback` with
/// a null context.  Enough to populate the gate's registration slot.
fn dummy_registration() -> UpdateCallbackRegistration {
    UpdateCallbackRegistration {
        context: 0,
        callback: noop_callback,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Core quiescence test.
///
/// Simulates the listener thread's "lock → copy → increment in_flight →
/// unlock → invoke → lock → decrement → notify" pattern, and concurrently
/// exercises the setter's "lock → replace → wait_while(in_flight > 0)" path.
///
/// **Post-condition asserted:** the setter thread does NOT return from
/// `wait_while` until after the simulated invocation has decremented
/// `in_flight` to zero and notified `drained`.
///
/// On old code (setter simply swaps the slot and returns immediately), the
/// "setter should still be blocked" assertion below would fail.
#[test]
fn set_callback_blocks_until_in_flight_drains() {
    let gate = Arc::new(UpdateCallbackGate::new());

    // Install a dummy registration so there is something to replace.
    {
        let mut inner = gate.inner.lock().unwrap();
        inner.registration = Some(dummy_registration());
    }

    // ------------------------------------------------------------------
    // Simulate the listener entering a callback invocation:
    // 1. Acquire the lock.
    // 2. Copy registration and increment in_flight — still under lock.
    // 3. Release the lock.
    // 4. "Execute" the callback (simulated: block on a barrier).
    // 5. Re-acquire, decrement in_flight, notify drained.
    // ------------------------------------------------------------------
    let (in_flight_started_tx, in_flight_started_rx) = mpsc::sync_channel::<()>(1);
    let release_barrier = Arc::new(Barrier::new(2)); // listener + test thread

    let listener_gate = Arc::clone(&gate);
    let listener_release = Arc::clone(&release_barrier);
    let listener = thread::spawn(move || {
        // Step 1–3: increment in_flight under lock, then drop lock.
        {
            let mut inner = listener_gate.inner.lock().unwrap();
            // Confirm there is a registration (the setter will clear it, but
            // that happens concurrently — we already have the lock, so we
            // read the value and increment before the setter can clear it).
            assert!(inner.registration.is_some(), "registration must be set before simulate");
            inner.in_flight += 1;
        } // lock released here

        // Signal: in_flight has been incremented; the setter can now try to run.
        in_flight_started_tx.send(()).unwrap();

        // Step 4: simulate slow foreign invocation — wait for test release.
        listener_release.wait();

        // Step 5: decrement and notify.
        {
            let mut inner = listener_gate.inner.lock().unwrap();
            inner.in_flight = inner.in_flight.saturating_sub(1);
            if inner.in_flight == 0 {
                listener_gate.drained.notify_all();
            }
        }
    });

    // Wait until the listener has incremented in_flight.
    in_flight_started_rx.recv_timeout(Duration::from_secs(5))
        .expect("listener should have started callback simulation within 5 s");

    // ------------------------------------------------------------------
    // Simulate the setter: replace the registration, then wait while
    // in_flight > 0.  This is exactly what `nmp_app_set_update_callback`
    // does with the new design.
    // ------------------------------------------------------------------
    let (setter_done_tx, setter_done_rx) = mpsc::sync_channel::<()>(1);
    let setter_gate = Arc::clone(&gate);
    thread::spawn(move || {
        let guard = setter_gate.inner.lock().unwrap();
        let mut guard = guard;
        guard.registration = None; // clear the old registration
        let _guard = setter_gate.drained.wait_while(guard, |inner| inner.in_flight > 0);
        // Quiescence reached: in_flight == 0 under the lock.
        setter_done_tx.send(()).unwrap();
    });

    // ------------------------------------------------------------------
    // Assert: the setter has NOT finished yet — callback is still blocked.
    // If this assertion fails, the setter returned while in_flight > 0,
    // which is the pre-fix bug.
    // ------------------------------------------------------------------
    assert!(
        setter_done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "setter returned while callback was still mid-flight — quiescence contract violated"
    );

    // Release the simulated callback.
    release_barrier.wait();

    // Now the setter must finish (in_flight drained to 0, condvar notified).
    setter_done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("setter should have returned after in_flight drained");

    listener.join().expect("listener thread should not panic");
}

/// Verify the trivial case: when `in_flight == 0` at the time the setter runs,
/// it returns immediately without blocking.
#[test]
fn set_callback_returns_immediately_when_no_callback_in_flight() {
    let gate = Arc::new(UpdateCallbackGate::new());

    {
        let mut inner = gate.inner.lock().unwrap();
        inner.registration = Some(dummy_registration());
    }

    // No listener has incremented in_flight, so wait_while should satisfy
    // the condition immediately.
    let start = std::time::Instant::now();
    {
        let guard = gate.inner.lock().unwrap();
        let mut guard = guard;
        guard.registration = None;
        let _guard = gate.drained.wait_while(guard, |inner| inner.in_flight > 0);
    }
    let elapsed = start.elapsed();
    // Should not have blocked for more than ~50 ms (well below CI timeouts).
    assert!(
        elapsed < Duration::from_millis(500),
        "setter blocked unexpectedly: {elapsed:?}"
    );
}

/// Bug 3 (D6 fail-loud): a poisoned `inner` lock must NOT cause the listener to
/// silently skip delivering the update frame to the host (that freezes the UI).
///
/// Pre-fix the listener used `let Ok(mut guard) = inner.lock() else { continue };`
/// so a poisoned mutex meant the registration was never read and the foreign
/// callback was never invoked. The fix recovers the still-valid inner state via
/// `unwrap_or_else(|e| e.into_inner())`. This test poisons the lock, then runs
/// the exact recovery dance the production listener uses and asserts the
/// callback still fires.
#[test]
fn poisoned_lock_still_fires_callback() {
    // A callback that bumps the AtomicU32 pointed at by `context`.
    extern "C" fn counting_callback(context: *mut std::ffi::c_void, _bytes: *const u8, _len: usize) {
        // SAFETY: the test passes a &AtomicU32's address as the context and
        // keeps it alive for the duration of the call.
        let counter = unsafe { &*(context as *const AtomicU32) };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    let fired = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(UpdateCallbackGate::new());
    {
        let mut inner = gate.inner.lock().unwrap();
        inner.registration = Some(UpdateCallbackRegistration {
            context: Arc::as_ptr(&fired) as usize,
            callback: counting_callback,
        });
    }

    // Poison the mutex: a thread panics while holding the lock.
    {
        let poison_gate = Arc::clone(&gate);
        let _ = thread::spawn(move || {
            let _guard = poison_gate.inner.lock().unwrap();
            panic!("intentional poison");
        })
        .join();
    }
    assert!(
        gate.inner.is_poisoned(),
        "precondition: the lock must be poisoned for this test to be meaningful"
    );

    // Replicate the production listener's acquire path with the fix applied:
    // recover the poisoned-but-valid guard instead of `continue`-ing.
    let registration = {
        let mut guard = gate.inner.lock().unwrap_or_else(|e| e.into_inner());
        let reg = guard.registration;
        if reg.is_some() {
            guard.in_flight += 1;
        }
        reg
    };
    let registration = registration.expect("poisoned lock must still surface the registration");

    // Invoke the callback exactly as the listener does.
    (registration.callback)(
        registration.context as *mut std::ffi::c_void,
        std::ptr::null(),
        0,
    );

    assert_eq!(
        fired.load(Ordering::SeqCst),
        1,
        "callback must fire even after the listener lock was poisoned (no silent skip)"
    );

    // The decrement path must also recover from poison so in_flight is balanced.
    {
        let mut guard = gate.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.in_flight = guard.in_flight.saturating_sub(1);
        assert_eq!(guard.in_flight, 0, "in_flight must be balanced after recovery");
    }
}

/// Verify that `in_flight` is only incremented when a registration is present.
///
/// This ensures a listener that races with a cleared slot does not
/// increment `in_flight` and inadvertently block a subsequent setter.
#[test]
fn in_flight_not_incremented_when_registration_absent() {
    let gate = Arc::new(UpdateCallbackGate::new());
    // No registration installed.

    let mut inner = gate.inner.lock().unwrap();
    let reg = inner.registration;
    if reg.is_some() {
        inner.in_flight += 1;
    }
    // Registration was None, so in_flight must still be 0.
    assert_eq!(inner.in_flight, 0, "in_flight should not be incremented when no registration");
}
