//! Kernel event observer slot (T146).
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

use crate::substrate::KernelEvent;
use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Mutex};

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
/// id allocator. Private — callers go through [`KernelEventObserverSlot`]'s
/// `register_*` / `unregister` methods.
pub struct ObserverInner {
    rust: Vec<(KernelEventObserverId, Arc<dyn KernelEventObserver>)>,
    c_abi: Vec<(KernelEventObserverId, KernelEventObserverRegistration)>,
    next_id: u64,
}

impl ObserverInner {
    fn new() -> Self {
        Self {
            rust: Vec::new(),
            c_abi: Vec::new(),
            next_id: 1,
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

/// Construct an empty slot. Called once in `nmp_app_new`.
pub fn new_event_observer_slot() -> KernelEventObserverSlot {
    Arc::new(Mutex::new(ObserverInner::new()))
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
/// The C-ABI payload is JSON — serialized once and shared across every C
/// observer. Serialization failure is a D6 silent no-op (no C observers fire
/// for this event; Rust observers still see the typed event).
pub fn notify_observers(slot: &KernelEventObserverSlot, event: &KernelEvent) {
    let (rust_snapshot, c_snapshot) = {
        let Ok(guard) = slot.lock() else {
            return;
        };
        if guard.rust.is_empty() && guard.c_abi.is_empty() {
            return;
        }
        (
            guard.rust.iter().map(|(_, o)| Arc::clone(o)).collect::<Vec<_>>(),
            guard.c_abi.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        )
    };

    for observer in &rust_snapshot {
        observer.on_kernel_event(event);
    }

    if !c_snapshot.is_empty() {
        let Ok(payload) = serde_json::to_string(event) else {
            return;
        };
        let Ok(cstr) = CString::new(payload) else {
            return;
        };
        for registration in &c_snapshot {
            (registration.callback)(registration.context as *mut c_void, cstr.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

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
        assert_eq!(C_CALLS.load(Ordering::SeqCst), 3);
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
        assert_eq!(obs.0.load(Ordering::SeqCst), 1);
        assert_eq!(C_CALLS.load(Ordering::SeqCst), 1);
    }
}
