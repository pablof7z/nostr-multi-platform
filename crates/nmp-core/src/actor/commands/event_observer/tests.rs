use super::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static C_CALLS: AtomicU32 = AtomicU32::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

extern "C" fn c_observer_shim(_ctx: *mut c_void, _payload: *const c_char) {
    C_CALLS.fetch_add(1, Ordering::SeqCst);
}

struct CountingObserver(AtomicU32);
impl KernelEventObserver for CountingObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn event() -> KernelEvent {
    KernelEvent {
        id: "id".into(),
        author: "auth".into(),
        kind: 1,
        created_at: 1,
        tags: vec![],
        content: "hi".into(),
    }
}

/// Block until `cond` holds or `timeout` elapses. C-ABI observers fire on
/// the per-slot drain thread, so assertions on their side effects must
/// poll rather than read immediately after `notify_observers`.
fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::yield_now();
    }
    cond()
}

#[test]
fn rust_observer_fires_per_event() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    register_rust_observer(&slot, obs.clone());
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());
    assert_eq!(obs.0.load(Ordering::SeqCst), 2);
}

#[test]
fn c_observer_fires_per_event() {
    let _g = SERIAL.lock().unwrap();
    C_CALLS.store(0, Ordering::SeqCst);
    let slot = new_event_observer_slot();
    register_c_observer(
        &slot,
        KernelEventObserverRegistration {
            context: 0,
            callback: c_observer_shim,
        },
    );
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());
    // C-ABI observers fire on the per-slot drain thread — poll.
    assert!(
        wait_until(Duration::from_secs(5), || C_CALLS.load(Ordering::SeqCst)
            == 3),
        "all three C-ABI callbacks must eventually fire (got {})",
        C_CALLS.load(Ordering::SeqCst)
    );
}

#[test]
fn notify_does_not_block_on_slow_c_observer() {
    // The actor-thread decoupling invariant: a slow foreign callback must
    // NOT delay `notify_observers`. A synchronous fan-out of this 200ms
    // callback would make `notify_observers` take >200ms; the channel
    // hand-off makes it return in well under that.
    static SLOW_CALLS: AtomicU32 = AtomicU32::new(0);
    extern "C" fn slow_shim(_ctx: *mut c_void, _payload: *const c_char) {
        std::thread::sleep(Duration::from_millis(200));
        SLOW_CALLS.fetch_add(1, Ordering::SeqCst);
    }
    let _g = SERIAL.lock().unwrap();
    SLOW_CALLS.store(0, Ordering::SeqCst);
    let slot = new_event_observer_slot();
    register_c_observer(
        &slot,
        KernelEventObserverRegistration {
            context: 0,
            callback: slow_shim,
        },
    );
    let started = Instant::now();
    notify_observers(&slot, &event());
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "notify_observers must return immediately, not block on the \
         200ms callback — took {elapsed:?}"
    );
    // The callback still runs, just off the actor thread.
    assert!(
        wait_until(Duration::from_secs(5), || SLOW_CALLS.load(Ordering::SeqCst)
            == 1),
        "slow callback must still fire on the drain thread"
    );
}

#[test]
fn unregister_stops_callbacks() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    let id = register_rust_observer(&slot, obs.clone());
    notify_observers(&slot, &event());
    unregister_observer(&slot, id);
    notify_observers(&slot, &event());
    notify_observers(&slot, &event());
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
}

#[test]
fn empty_slot_is_silent() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    // No registrations — should not panic, allocate, or do anything.
    notify_observers(&slot, &event());
}

/// D6 — a Rust observer that panics inside `on_kernel_event` must not
/// unwind the calling (actor) thread, must not stop sibling observers
/// from firing, and must stay registered so the next event still
/// reaches it. Mirrors the per-callback panic isolation in
/// `actor/mod.rs`'s relay-event lane: the outer actor `catch_unwind`
/// guards only the relay-event handler, NOT this fan-out, so each
/// in-process observer needs its own guard.
///
/// Without the `catch_unwind` around `observer.on_kernel_event(event)`
/// in `notify_observers`, this test aborts the process.
#[test]
fn panicking_rust_observer_isolated_from_siblings() {
    struct Boom;
    impl KernelEventObserver for Boom {
        fn on_kernel_event(&self, _event: &KernelEvent) {
            panic!("buggy rust observer");
        }
    }

    let _g = SERIAL.lock().unwrap();
    let slot = new_event_observer_slot();
    // Sibling observer registered AFTER the panicking one: with
    // per-observer `catch_unwind` the sibling still fires; without it,
    // the panic would unwind out of `notify_observers` and the
    // sibling's counter would stay at 0.
    register_rust_observer(&slot, Arc::new(Boom));
    let sibling = Arc::new(CountingObserver(AtomicU32::new(0)));
    register_rust_observer(&slot, sibling.clone());

    // First notification — Boom panics, sibling must still fire.
    notify_observers(&slot, &event());
    // Second notification — Boom is still registered (the panic did
    // not unregister it) and sibling fires again.
    notify_observers(&slot, &event());

    assert_eq!(
        sibling.0.load(Ordering::SeqCst),
        2,
        "sibling observer must fire on both events despite the panicking sibling"
    );
}

#[test]
fn mixed_rust_and_c_observers_both_fire() {
    let _g = SERIAL.lock().unwrap();
    C_CALLS.store(0, Ordering::SeqCst);
    let slot = new_event_observer_slot();
    let obs = Arc::new(CountingObserver(AtomicU32::new(0)));
    register_rust_observer(&slot, obs.clone());
    register_c_observer(
        &slot,
        KernelEventObserverRegistration {
            context: 0,
            callback: c_observer_shim,
        },
    );
    notify_observers(&slot, &event());
    // Rust observer is synchronous — assert immediately.
    assert_eq!(obs.0.load(Ordering::SeqCst), 1);
    // C-ABI observer fires on the drain thread — poll.
    assert!(
        wait_until(Duration::from_secs(5), || C_CALLS.load(Ordering::SeqCst)
            == 1),
        "C-ABI observer must fire (got {})",
        C_CALLS.load(Ordering::SeqCst)
    );
}
