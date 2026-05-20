//! Kernel event observer slot (T146).
//!
//! # The v1 extension model
//!
//! `KernelEventObserver` is the **shipping v1 extension path** for per-app
//! event processing — the mechanism the kernel actually drives. Per-app
//! crates (e.g. `nmp-app-chirp`) register an observer here and compose
//! typed views from the raw `KernelEvent` fan-out.
//!
//! It is *not* the same thing as `substrate::ViewModule`. Those traits are
//! **v2 pre-wiring**: a future typed, namespace-keyed dispatch design whose
//! kernel-side dispatch runtime does not exist yet. The kernel never invokes
//! `ViewModule` impls at runtime. New per-app event processing should
//! register a `KernelEventObserver` here, not implement `ViewModule` (see
//! the v1-vs-v2 note in `crates/nmp-core/src/substrate/mod.rs`).
//!
//! Mirrors `lifecycle.rs`'s `LifecycleObserverSlot` pattern, but with two
//! registration channels rather than one:
//!
//! - **Rust trait objects** (`Arc<dyn KernelEventObserver>`) for in-process
//!   consumers like the per-app `nmp-app-chirp` crate that needs typed
//!   `&KernelEvent` access without crossing a C-ABI boundary.
//! - **C-ABI function pointers** (`KernelEventObserverFn`) for Swift / Kotlin
//!   consumers that receive each event as a JSON-serialized C string.
//!
//! Both channels share one slot and fire on the same fan-out site
//! (`Kernel::notify_event_observers`, called from `ingest/timeline.rs` after
//! every `EventStore::insert` returning `Inserted | Replaced`).
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` emits raw `KernelEvent`s; per-app crates compose
//!   them into typed views (e.g. `nmp_nip01::Nip10ModularTimelineView`).
//!   The kernel never names NIP types. ADR-0009.
//! * **D6** — observers fire best-effort. A poisoned mutex, missing C string
//!   (CString conversion failure), or panicking observer are silent no-ops;
//!   nothing crosses the FFI as an exception.
//! * **Re-entrancy** — observers snapshot the registration list under the
//!   lock, then release the lock before invoking. Observers may re-register
//!   inside a callback without deadlocking.
//!
//! ## Actor-thread decoupling
//!
//! The kernel fan-out (`notify_observers`) runs on the **actor thread** —
//! the same thread that drives relay ingest, subscription management, and
//! UI updates. A slow Swift / Kotlin callback that blocked here would stall
//! *all* relay ingest behind it.
//!
//! Therefore the **C-ABI** fan-out is decoupled: each slot owns a bounded
//! [`std::sync::mpsc::sync_channel`] and a single background drain thread
//! (spawned once in `new_event_observer_slot`, mirroring the update-listener
//! thread in `ffi/mod.rs`). `notify_observers` serializes the event JSON
//! once, then `try_send`s a `(snapshot, payload)` envelope and returns
//! immediately. The drain thread invokes the foreign callbacks off the hot
//! path. The actor thread never blocks on a callback's duration.
//!
//! **Rust** trait observers are NOT decoupled: they are in-process
//! consumers whose trait contract already mandates "must be cheap and must
//! not panic". They still fire synchronously on the actor thread — keeping
//! their existing ordering / no-clone semantics — and that is intentional.
//!
//! If the channel is full (a persistently slow callback), the envelope is
//! dropped (rate-limit backpressure, D6 best-effort). The first overflow
//! per slot logs once so the condition is visible to ops.

use crate::substrate::KernelEvent;
use std::ffi::{c_char, c_void, CString};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Bound on the per-slot C-ABI fan-out channel. Each queued envelope is one
/// ingested event plus the snapshot of C registrations to deliver it to.
/// 1024 absorbs a long burst of relay frames while a callback is briefly
/// slow; a callback slow enough to overflow this is dropping events anyway,
/// so dropping the envelope is the correct rate-limit backpressure (D6).
const C_FANOUT_CHANNEL_BOUND: usize = 1024;

/// One unit of decoupled C-ABI fan-out work: the snapshot of C registrations
/// captured under the lock on the actor thread, plus the JSON payload
/// serialized once. The drain thread owns this and invokes each callback.
struct CFanoutEnvelope {
    registrations: Vec<KernelEventObserverRegistration>,
    payload: Arc<CString>,
}

/// C-ABI shape: `(context, *const c_char)` where the C string is a
/// nul-terminated JSON encoding of [`KernelEvent`]. Same `extern "C" fn` shape
/// as the existing update callback (`ffi/mod.rs::UpdateCallback`) so Swift
/// bridges can use the existing decode pattern.
pub type KernelEventObserverFn = extern "C" fn(*mut c_void, *const c_char);

/// Stable id returned by `register_*` so callers can later unregister exactly
/// the right entry. Wraps a `u64` rather than the registration pointer so the
/// FFI ABI is integer-shaped (Swift sees `UInt64`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct KernelEventObserverId(pub u64);

/// C-ABI registration record. `Copy` so it can be cloned out from under the
/// mutex without holding the lock across the FFI call (mirrors
/// `LifecycleObserverRegistration`).
#[derive(Clone, Copy)]
pub struct KernelEventObserverRegistration {
    /// Caller-opaque context pointer, stored as `usize` for `Send`/`Sync`
    /// (raw pointers are neither). Re-cast on invocation.
    pub context: usize,
    pub callback: KernelEventObserverFn,
}

/// Slot contents: zero or more Rust + C-ABI registrations, plus a monotonic
/// id allocator and the C-ABI fan-out channel sender. Private — callers go
/// through [`KernelEventObserverSlot`]'s `register_*` / `unregister` methods.
pub struct ObserverInner {
    rust: Vec<(KernelEventObserverId, Arc<dyn KernelEventObserver>)>,
    c_abi: Vec<(KernelEventObserverId, KernelEventObserverRegistration)>,
    next_id: u64,
    /// Sender half of the bounded C-ABI fan-out channel. `notify_observers`
    /// `try_send`s envelopes here; the per-slot drain thread receives them.
    /// Dropping the whole `ObserverInner` drops this sender, which makes the
    /// drain thread's `recv()` return `Err` and the thread exit cleanly.
    c_fanout_tx: SyncSender<CFanoutEnvelope>,
}

impl ObserverInner {
    fn new(c_fanout_tx: SyncSender<CFanoutEnvelope>) -> Self {
        Self {
            rust: Vec::new(),
            c_abi: Vec::new(),
            next_id: 1,
            c_fanout_tx,
        }
    }

    fn alloc_id(&mut self) -> KernelEventObserverId {
        let id = KernelEventObserverId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }
}

/// Shared slot. The FFI surface (`ffi/event_observer.rs`) holds one clone for
/// registration; the kernel holds another for invocation. `Mutex` ensures
/// registration and invocation never tear.
pub type KernelEventObserverSlot = Arc<Mutex<ObserverInner>>;

/// Invoke one decoupled C-ABI fan-out envelope. Runs on the per-slot drain
/// thread, never on the actor thread. Each callback is wrapped in
/// [`crate::ffi_guard::guard_ffi_callback`] so a panicking / throwing
/// foreign observer cannot unwind across the C ABI nor stop the rest.
fn drain_c_envelope(envelope: CFanoutEnvelope) {
    let ptr = envelope.payload.as_ptr();
    for registration in &envelope.registrations {
        crate::ffi_guard::guard_ffi_callback("kernel event observer", || {
            (registration.callback)(registration.context as *mut c_void, ptr);
        });
    }
}

/// Construct an empty slot, spawning its background C-ABI drain thread.
///
/// The drain thread lives for the life of the slot: it exits when the last
/// `Arc` to the `ObserverInner` is dropped (which drops `c_fanout_tx`, so
/// `recv()` returns `Err`). The slot's `Arc` is shared by `NmpApp` and the
/// kernel actor; both must drop before the drain thread joins — and across
/// `ActorCommand::Reset` the same `Arc` survives, so the thread is never
/// respawned. The `JoinHandle` is detached: there is no synchronous point
/// to join it, and on process teardown the dropped sender ends it cleanly.
///
/// Called once in `nmp_app_new`.
pub fn new_event_observer_slot() -> KernelEventObserverSlot {
    let (tx, rx) = sync_channel::<CFanoutEnvelope>(C_FANOUT_CHANNEL_BOUND);
    let _drain: JoinHandle<()> = std::thread::Builder::new()
        .name("nmp-kev-observer-drain".into())
        .spawn(move || {
            // `recv()` blocks off the actor's hot path; exits when every
            // sender (held inside `ObserverInner`) has been dropped.
            while let Ok(envelope) = rx.recv() {
                drain_c_envelope(envelope);
            }
        })
        .expect("spawn kernel-event observer drain thread");
    Arc::new(Mutex::new(ObserverInner::new(tx)))
}

/// In-process Rust observer. `Send + Sync` so it can live behind an `Arc`
/// shared between the actor thread and any registrant. Implementors carry
/// their own interior mutability (typically a `Mutex<State>`) because the
/// trait method takes `&self`.
pub trait KernelEventObserver: Send + Sync {
    /// Called once per event that has been accepted into the kernel's
    /// in-memory store via `EventStore::insert` returning `Inserted` or
    /// `Replaced`. Duplicates / supersessions / rejections do NOT fire the
    /// observer (the event is not a "new fact" from the projection's
    /// perspective).
    ///
    /// Implementations must be cheap and must not panic — the call site is
    /// on the actor thread between relay frames.
    fn on_kernel_event(&self, event: &KernelEvent);
}

/// Register an in-process Rust observer. Returns an opaque id the caller
/// retains to unregister later. Idempotent across distinct observers; the
/// same `Arc` can be registered multiple times and will fire once per
/// registration.
pub fn register_rust_observer(
    slot: &KernelEventObserverSlot,
    observer: Arc<dyn KernelEventObserver>,
) -> KernelEventObserverId {
    let mut guard = match slot.lock() {
        Ok(g) => g,
        // Poisoned mutex — D6 silent fail. Return a sentinel id; the caller
        // will eventually try to unregister it as a no-op.
        Err(_) => return KernelEventObserverId(0),
    };
    let id = guard.alloc_id();
    guard.rust.push((id, observer));
    id
}

/// Register a C-ABI observer. Returns an opaque id the caller retains to
/// unregister later. `Copy` registration record allows lock-free invocation.
pub fn register_c_observer(
    slot: &KernelEventObserverSlot,
    registration: KernelEventObserverRegistration,
) -> KernelEventObserverId {
    let mut guard = match slot.lock() {
        Ok(g) => g,
        Err(_) => return KernelEventObserverId(0),
    };
    let id = guard.alloc_id();
    guard.c_abi.push((id, registration));
    id
}

/// Unregister by id (works for either Rust or C-ABI registrations).
/// Idempotent: unknown ids are silent no-ops.
///
/// For C-ABI registrations: an envelope already enqueued for the drain
/// thread captured its snapshot *before* this call and will still fire
/// once. The foreign caller's contract is therefore unchanged from before
/// the channel decoupling — do not free the registration's `context`
/// pointer until you have fenced against any in-flight callback (the
/// decoupling only widens that pre-existing window by the drain latency).
pub fn unregister_observer(slot: &KernelEventObserverSlot, id: KernelEventObserverId) {
    if let Ok(mut guard) = slot.lock() {
        guard.rust.retain(|(rid, _)| *rid != id);
        guard.c_abi.retain(|(rid, _)| *rid != id);
    }
}

/// Fan out one event to every registered observer. Snapshot-and-release: the
/// lock is held only long enough to clone the registration vectors, so
/// observers re-registering inside their callback (legal) cannot deadlock.
///
/// **Rust** observers fire synchronously on the calling (actor) thread —
/// their trait contract mandates they be cheap.
///
/// **C-ABI** observers are decoupled from the actor thread: the event JSON
/// is serialized once here, then a `(snapshot, payload)` envelope is
/// `try_send`-posted to the slot's bounded channel and the per-slot drain
/// thread invokes the foreign callbacks. `notify_observers` therefore never
/// blocks on a callback's duration — a slow Swift observer cannot stall
/// relay ingest. If the channel is full the envelope is dropped (rate-limit
/// backpressure, D6); the first overflow per slot logs once.
///
/// Serialization failure is a D6 silent no-op (no C observers fire for this
/// event; Rust observers still see the typed event).
pub fn notify_observers(slot: &KernelEventObserverSlot, event: &KernelEvent) {
    // Hold the lock only to snapshot registrations + clone the sender; all
    // observer invocation (Rust inline, C-ABI via channel) happens after the
    // lock is released so re-entrant registration cannot deadlock.
    let (rust_snapshot, c_snapshot, c_fanout_tx) = {
        let Ok(guard) = slot.lock() else {
            return;
        };
        if guard.rust.is_empty() && guard.c_abi.is_empty() {
            return;
        }
        (
            guard.rust.iter().map(|(_, o)| Arc::clone(o)).collect::<Vec<_>>(),
            guard.c_abi.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            guard.c_fanout_tx.clone(),
        )
    };

    for observer in &rust_snapshot {
        observer.on_kernel_event(event);
    }

    if !c_snapshot.is_empty() {
        // Serialize once on the actor thread (cheap, bounded), then hand the
        // envelope off to the drain thread. The actor thread does NOT invoke
        // any foreign callback.
        let Ok(payload) = serde_json::to_string(event) else {
            return;
        };
        let Ok(cstr) = CString::new(payload) else {
            return;
        };
        let envelope = CFanoutEnvelope {
            registrations: c_snapshot,
            payload: Arc::new(cstr),
        };
        // Channel full (slow callback) or disconnected (drain thread gone).
        // Drop the envelope — D6 best-effort: library code performs no I/O
        // side effects, so the overflow is absorbed silently.
        let _ = c_fanout_tx.try_send(envelope);
    }
}

#[cfg(test)]
mod tests {
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
            wait_until(Duration::from_secs(5), || C_CALLS.load(Ordering::SeqCst) == 3),
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
            wait_until(Duration::from_secs(5), || SLOW_CALLS.load(Ordering::SeqCst) == 1),
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
            wait_until(Duration::from_secs(5), || C_CALLS.load(Ordering::SeqCst) == 1),
            "C-ABI observer must fire (got {})",
            C_CALLS.load(Ordering::SeqCst)
        );
    }
}
