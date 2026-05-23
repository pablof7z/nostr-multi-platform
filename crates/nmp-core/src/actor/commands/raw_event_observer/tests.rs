use super::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static C_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_KIND: AtomicU32 = AtomicU32::new(0);
static SERIAL: Mutex<()> = Mutex::new(());
static STALE_BLOCK_STARTED_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
static STALE_BLOCK_RELEASE_RX: OnceLock<Mutex<Option<Receiver<()>>>> = OnceLock::new();
static STALE_DRAINED_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
static STALE_TARGET_CALLS: AtomicU32 = AtomicU32::new(0);

/// Block until `cond` holds or `timeout` elapses. C-ABI raw observers
/// fire on the per-slot drain thread, so assertions on their side
/// effects must poll rather than read immediately after
/// `notify_raw_observers`.
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

extern "C" fn c_observer_shim(_ctx: *mut c_void, payload: *const c_char) {
    C_CALLS.fetch_add(1, Ordering::SeqCst);
    if !payload.is_null() {
        // SAFETY: callback contract — borrowed nul-terminated C string.
        let s = unsafe { std::ffi::CStr::from_ptr(payload) };
        if let Ok(json) = s.to_str() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
                if let Some(k) = v.get("kind").and_then(|k| k.as_u64()) {
                    LAST_KIND.store(k as u32, Ordering::SeqCst);
                }
            }
        }
    }
}

fn set_stale_block_started(tx: Option<Sender<()>>) {
    *STALE_BLOCK_STARTED_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = tx;
}

fn set_stale_block_release(rx: Option<Receiver<()>>) {
    *STALE_BLOCK_RELEASE_RX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = rx;
}

fn set_stale_drained(tx: Option<Sender<()>>) {
    *STALE_DRAINED_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = tx;
}

extern "C" fn stale_blocking_shim(_ctx: *mut c_void, _payload: *const c_char) {
    if let Some(slot) = STALE_BLOCK_STARTED_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
    if let Some(slot) = STALE_BLOCK_RELEASE_RX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(rx) = guard.as_ref() {
                let _ = rx.recv();
            }
        }
    }
}

extern "C" fn stale_target_shim(_ctx: *mut c_void, _payload: *const c_char) {
    STALE_TARGET_CALLS.fetch_add(1, Ordering::SeqCst);
}

extern "C" fn stale_marker_shim(_ctx: *mut c_void, _payload: *const c_char) {
    if let Some(slot) = STALE_DRAINED_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

struct CapturingObserver(Mutex<Vec<(u32, String)>>);
impl RawEventObserver for CapturingObserver {
    fn on_raw_event(&self, kind: u32, json: &str) {
        self.0.lock().unwrap().push((kind, json.to_string()));
    }
}

fn raw(id: &str, kind: u32) -> RawEvent {
    RawEvent {
        id: id.into(),
        pubkey: "ab".repeat(32),
        created_at: 1700000000,
        kind,
        tags: vec![vec!["t".into(), "x".into()]],
        content: "hello".into(),
        sig: "cd".repeat(64),
    }
}

#[test]
fn raw_event_json_has_nip01_field_order() {
    // The Chirp ingest agent depends on this byte-faithful order.
    let json = serde_json::to_string(&raw("deadbeef", 1)).unwrap();
    let pos = |k: &str| json.find(k).unwrap();
    assert!(
        pos("\"id\"") < pos("\"pubkey\"")
            && pos("\"pubkey\"") < pos("\"created_at\"")
            && pos("\"created_at\"") < pos("\"kind\"")
            && pos("\"kind\"") < pos("\"tags\"")
            && pos("\"tags\"") < pos("\"content\"")
            && pos("\"content\"") < pos("\"sig\""),
        "field order must be id,pubkey,created_at,kind,tags,content,sig — got {json}"
    );
    assert!(
        json.contains("\"sig\":\"cdcd"),
        "sig must be present verbatim"
    );
}

#[test]
fn rust_observer_receives_verbatim_json() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    let obs = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
    register_rust_raw_observer(&slot, KindFilter::default(), obs.clone());
    notify_raw_observers(&slot, &raw("aa", 1), None);
    let captured = obs.0.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, 1);
    let v: serde_json::Value = serde_json::from_str(&captured[0].1).unwrap();
    assert_eq!(v["sig"], "cd".repeat(64));
    assert_eq!(v["id"], "aa");
}

#[test]
fn kind_filter_excludes_non_matching() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    let obs = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
    register_rust_raw_observer(&slot, KindFilter::from_kinds([445u32]), obs.clone());
    notify_raw_observers(&slot, &raw("k1", 1), None); // filtered out
    notify_raw_observers(&slot, &raw("k445", 445), None); // delivered
    let captured = obs.0.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, 445);
}

#[test]
fn idle_fast_path_tracks_filter() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    assert!(raw_observers_idle_for_kind(&slot, 1));
    let obs = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
    let id = register_rust_raw_observer(&slot, KindFilter::from_kinds([7u32]), obs);
    assert!(
        raw_observers_idle_for_kind(&slot, 1),
        "kind 1 not registered"
    );
    assert!(!raw_observers_idle_for_kind(&slot, 7), "kind 7 registered");
    unregister_raw_observer(&slot, id);
    assert!(raw_observers_idle_for_kind(&slot, 7), "unregistered → idle");
}

#[test]
fn c_observer_fires_with_filter() {
    let _g = SERIAL.lock().unwrap();
    C_CALLS.store(0, Ordering::SeqCst);
    LAST_KIND.store(0, Ordering::SeqCst);
    let slot = new_raw_event_observer_slot();
    register_c_raw_observer(
        &slot,
        RawEventObserverRegistration {
            context: 0,
            callback: c_observer_shim,
            kinds: KindFilter::from_kinds([1059u32]),
        },
    );
    notify_raw_observers(&slot, &raw("nope", 1), None); // filtered
    notify_raw_observers(&slot, &raw("yes", 1059), None); // delivered
                                                          // C-ABI observers fire on the per-slot drain thread — poll on the
                                                          // LAST side effect (`LAST_KIND`, written after `C_CALLS`) so the
                                                          // wait does not race ahead of the callback body completing.
    assert!(
        wait_until(Duration::from_secs(5), || {
            LAST_KIND.load(Ordering::SeqCst) == 1059
        }),
        "delivered kind:1059 callback must run on the drain thread"
    );
    assert_eq!(
        C_CALLS.load(Ordering::SeqCst),
        1,
        "exactly one C-ABI callback (the kind:1059 one; kind:1 filtered)"
    );
}

#[test]
fn notify_raw_does_not_block_on_slow_c_observer() {
    // Actor-thread decoupling invariant: a slow foreign callback must NOT
    // delay `notify_raw_observers`.
    static SLOW_CALLS: AtomicU32 = AtomicU32::new(0);
    extern "C" fn slow_shim(_ctx: *mut c_void, _payload: *const c_char) {
        std::thread::sleep(Duration::from_millis(200));
        SLOW_CALLS.fetch_add(1, Ordering::SeqCst);
    }
    let _g = SERIAL.lock().unwrap();
    SLOW_CALLS.store(0, Ordering::SeqCst);
    let slot = new_raw_event_observer_slot();
    register_c_raw_observer(
        &slot,
        RawEventObserverRegistration {
            context: 0,
            callback: slow_shim,
            kinds: KindFilter::default(),
        },
    );
    let started = Instant::now();
    notify_raw_observers(&slot, &raw("slow", 1), None);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "notify_raw_observers must return immediately, not block on the \
         200ms callback — took {elapsed:?}"
    );
    assert!(
        wait_until(Duration::from_secs(5), || SLOW_CALLS.load(Ordering::SeqCst)
            == 1),
        "slow callback must still fire on the drain thread"
    );
}

#[test]
fn unregister_fences_queued_c_callback_stale_delivery() {
    let _g = SERIAL.lock().unwrap();
    STALE_TARGET_CALLS.store(0, Ordering::SeqCst);
    let (started_tx, started_rx) = channel::<()>();
    let (release_tx, release_rx) = channel::<()>();
    let (drained_tx, drained_rx) = channel::<()>();
    set_stale_block_started(Some(started_tx));
    set_stale_block_release(Some(release_rx));
    set_stale_drained(Some(drained_tx));

    let slot = new_raw_event_observer_slot();
    register_c_raw_observer(
        &slot,
        RawEventObserverRegistration {
            context: 0,
            callback: stale_blocking_shim,
            kinds: KindFilter::default(),
        },
    );
    let target_id = register_c_raw_observer(
        &slot,
        RawEventObserverRegistration {
            context: 0,
            callback: stale_target_shim,
            kinds: KindFilter::default(),
        },
    );
    register_c_raw_observer(
        &slot,
        RawEventObserverRegistration {
            context: 0,
            callback: stale_marker_shim,
            kinds: KindFilter::default(),
        },
    );

    notify_raw_observers(&slot, &raw("queued", 1), None);
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("blocking callback must start");
    unregister_raw_observer(&slot, target_id);
    release_tx.send(()).expect("release blocking callback");
    drained_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("marker callback proves the queued envelope drained");
    assert_eq!(
        STALE_TARGET_CALLS.load(Ordering::SeqCst),
        0,
        "a C callback already snapshotted into a queued envelope must not fire after unregister"
    );

    set_stale_block_started(None);
    set_stale_block_release(None);
    set_stale_drained(None);
}

#[test]
fn unregister_stops_callbacks() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    let obs = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
    let id = register_rust_raw_observer(&slot, KindFilter::default(), obs.clone());
    notify_raw_observers(&slot, &raw("a", 1), None);
    unregister_raw_observer(&slot, id);
    notify_raw_observers(&slot, &raw("b", 1), None);
    assert_eq!(obs.0.lock().unwrap().len(), 1);
}

#[test]
fn empty_slot_is_silent() {
    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    notify_raw_observers(&slot, &raw("a", 1), None); // no panic, no-op
}

/// D6 — a Rust raw observer that panics inside `on_raw_event` must not
/// unwind the calling (actor) thread, must not stop sibling observers
/// from firing, and must stay registered for subsequent events.
/// Mirrors the equivalent invariant for the `KernelEventObserver` slot.
///
/// Without the `catch_unwind` around `observer.on_raw_event(...)` in
/// `notify_raw_observers`, this test aborts the process.
#[test]
fn panicking_rust_observer_isolated_from_siblings() {
    struct Boom;
    impl RawEventObserver for Boom {
        fn on_raw_event(&self, _kind: u32, _json: &str) {
            panic!("buggy rust raw observer");
        }
    }

    let _g = SERIAL.lock().unwrap();
    let slot = new_raw_event_observer_slot();
    register_rust_raw_observer(&slot, KindFilter::default(), Arc::new(Boom));
    let sibling = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
    register_rust_raw_observer(&slot, KindFilter::default(), sibling.clone());

    notify_raw_observers(&slot, &raw("e1", 1), None);
    notify_raw_observers(&slot, &raw("e2", 1), None);

    let captured = sibling.0.lock().unwrap();
    assert_eq!(
        captured.len(),
        2,
        "sibling raw observer must fire on both events despite the panicking sibling"
    );
}
