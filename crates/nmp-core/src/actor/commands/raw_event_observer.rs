//! Raw signed-event observer slot.
//!
//! A generic, additive tap that delivers INBOUND verbatim-signed Nostr
//! events — the flat NIP-01 object `{id, pubkey, created_at, kind, tags,
//! content, sig}` *including the `sig`* — to a registered consumer, after
//! the kernel's existing Schnorr + id-hash gate and store provenance path
//! have accepted the event.
//!
//! This is deliberately separate from the `KernelEventObserver` slot
//! (`event_observer.rs`): that one emits the sig-stripped, projection-stable
//! `KernelEvent`. Some consumers need the *whole* signed event verbatim
//! (the inbound-ingest seam where a protocol crate must hand the full
//! `nostr::Event` to its own state machine). Mutating `KernelEvent` to add
//! `sig` would couple every projection consumer to that need; a parallel
//! tap keeps the projection type stable and the new capability additive.
//!
//! Two registration channels mirror `event_observer.rs`:
//!
//! - **Rust trait objects** (`Arc<dyn RawEventObserver>`) for in-process
//!   consumers (per-app crates) that want the verbatim JSON without a
//!   C-ABI hop.
//! - **C-ABI function pointers** (`RawEventObserverFn`) for Swift / Kotlin
//!   consumers that receive each event as a JSON-serialized C string.
//!
//! Each registration carries an optional kind filter (a set of u32 kinds).
//! An empty filter means "deliver every kind".
//! Unregistering an id deactivates its per-registration lifecycle before
//! queued C-ABI envelopes drain, so stale callbacks are skipped and any
//! already in-flight callback is fenced before unregister returns.
//!
//! ## Doctrine
//!
//! * **D0** — generic capability. The kernel never names a NIP / protocol;
//!   the symbol set is `RawEvent*`, no app or higher-protocol
//!   nouns. Any consumer can register a raw tap.
//! * **D6** — observers fire best-effort. A poisoned mutex, missing C
//!   string (`CString` conversion failure), or panicking observer are silent
//!   no-ops; nothing crosses the FFI as an exception.
//! * **Re-entrancy** — observers snapshot the registration list under the
//!   lock, then release the lock before invoking. Observers may
//!   re-register inside a callback without deadlocking.
//! * **C-string lifetime** — the `*const c_char` payload is borrowed for
//!   the duration of the callback only; consumers must copy any bytes they
//!   need. Same contract as `event_observer.rs` / `ffi/mod.rs`.
//!
//! ## Actor-thread decoupling
//!
//! `notify_raw_observers` runs on the **actor thread** — the same thread
//! that drives relay ingest. A slow Swift / Kotlin callback blocking here
//! would stall all relay ingest. So the **C-ABI** fan-out is decoupled
//! exactly like `event_observer.rs`: the slot owns a bounded
//! [`std::sync::mpsc::sync_channel`] and a single background drain thread
//! (spawned in `new_raw_event_observer_slot`). `notify_raw_observers`
//! serializes the verbatim JSON once, `try_send`s a `(snapshot, payload)`
//! envelope, and returns immediately. **Rust** trait observers stay
//! synchronous on the actor thread — their trait contract already mandates
//! "must be cheap and must not panic". On channel overflow the envelope is
//! dropped silently (D6 backpressure — library code performs no I/O).

use crate::store::RawEvent;
use std::collections::BTreeSet;
use std::ffi::{c_char, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{JoinHandle, ThreadId};

/// Bound on the per-slot C-ABI fan-out channel. See the equivalent constant
/// in `event_observer.rs` for the rationale.
const C_FANOUT_CHANNEL_BOUND: usize = 1024;

/// One unit of decoupled C-ABI raw fan-out work: the snapshot of matching C
/// registrations captured under the lock, plus the verbatim NIP-01 JSON
/// serialized once. The drain thread owns this and invokes each callback.
struct CRawFanoutEnvelope {
    registrations: Vec<Arc<RawCObserverEntry>>,
    payload: Arc<CString>,
}

/// C-ABI shape: `(context, *const c_char)` where the C string is a
/// nul-terminated JSON encoding of the verbatim signed event
/// `{id, pubkey, created_at, kind, tags, content, sig}`. Same `extern "C"
/// fn` shape as `KernelEventObserverFn` so Swift bridges reuse the existing
/// decode pattern.
pub type RawEventObserverFn = extern "C" fn(*mut c_void, *const c_char);

/// Stable id returned by `register_*` so callers can later unregister
/// exactly the right entry. Integer-shaped ABI (Swift sees `UInt64`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct RawEventObserverId(pub u64);

/// Per-registration kind filter. Empty → match every kind.
#[derive(Clone, Debug, Default)]
pub struct KindFilter(BTreeSet<u32>);

impl KindFilter {
    /// Build a filter from a kind list. An empty list yields the
    /// match-everything filter.
    #[must_use]
    pub fn from_kinds<I: IntoIterator<Item = u32>>(kinds: I) -> Self {
        Self(kinds.into_iter().collect())
    }

    /// `true` if `kind` should be delivered: either the filter is empty
    /// (match all) or `kind` is explicitly listed.
    #[must_use]
    pub fn matches(&self, kind: u32) -> bool {
        self.0.is_empty() || self.0.contains(&kind)
    }

    /// `true` when no kinds are listed (match-everything).
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.is_empty()
    }
}

/// C-ABI registration record. Not `Copy` (the `KindFilter` owns a set), so
/// invocation clones the snapshot vector under the lock then releases it.
#[derive(Clone)]
pub struct RawEventObserverRegistration {
    /// Caller-opaque context pointer, stored as `usize` for `Send`/`Sync`
    /// (raw pointers are neither). Re-cast on invocation.
    pub context: usize,
    pub callback: RawEventObserverFn,
    /// Kinds this registration wants; empty → all kinds.
    pub kinds: KindFilter,
}

struct RawObserverLifecycle {
    state: Mutex<RawObserverLifecycleState>,
    idle: Condvar,
}

struct RawObserverLifecycleState {
    active: bool,
    in_flight: usize,
    callers: Vec<ThreadId>,
}

struct RawObserverCallGuard<'a> {
    lifecycle: &'a RawObserverLifecycle,
}

impl RawObserverLifecycle {
    fn new() -> Self {
        Self {
            state: Mutex::new(RawObserverLifecycleState {
                active: true,
                in_flight: 0,
                callers: Vec::new(),
            }),
            idle: Condvar::new(),
        }
    }

    fn begin(&self) -> Option<RawObserverCallGuard<'_>> {
        let mut state = self.state.lock().ok()?;
        if !state.active {
            return None;
        }
        state.in_flight = state.in_flight.saturating_add(1);
        state.callers.push(std::thread::current().id());
        Some(RawObserverCallGuard { lifecycle: self })
    }

    fn deactivate_and_wait(&self) {
        let current = std::thread::current().id();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.active = false;
        while state.in_flight > 0 && !state.callers.contains(&current) {
            let Ok(next) = self.idle.wait(state) else {
                return;
            };
            state = next;
        }
    }
}

impl Drop for RawObserverCallGuard<'_> {
    fn drop(&mut self) {
        let Ok(mut state) = self.lifecycle.state.lock() else {
            return;
        };
        state.in_flight = state.in_flight.saturating_sub(1);
        let current = std::thread::current().id();
        if let Some(index) = state.callers.iter().position(|id| *id == current) {
            state.callers.swap_remove(index);
        }
        if state.in_flight == 0 {
            self.lifecycle.idle.notify_all();
        }
    }
}

struct RawRustObserverEntry {
    id: RawEventObserverId,
    kinds: KindFilter,
    observer: Arc<dyn RawEventObserver>,
    lifecycle: Arc<RawObserverLifecycle>,
}

struct RawCObserverEntry {
    id: RawEventObserverId,
    registration: RawEventObserverRegistration,
    lifecycle: Arc<RawObserverLifecycle>,
}

/// In-process Rust observer. `Send + Sync` so it can live behind an `Arc`
/// shared between the actor thread and any registrant.
pub trait RawEventObserver: Send + Sync {
    /// Called once per accepted inbound event whose kind matches this
    /// observer's registered filter. `json` is the verbatim flat NIP-01
    /// signed-event JSON (`{id, pubkey, created_at, kind, tags, content,
    /// sig}`). Implementations must be cheap and must not panic — the call
    /// site is on the actor thread between relay frames.
    fn on_raw_event(&self, kind: u32, json: &str);

    /// Source-aware variant used by the kernel after the event has passed
    /// store insertion. `source_relay_url` is the delivering relay URL that
    /// was persisted as store provenance. Existing observers that only need
    /// the verbatim event can implement [`Self::on_raw_event`] and inherit
    /// this forwarding default.
    fn on_raw_event_with_source(&self, kind: u32, json: &str, _source_relay_url: Option<&str>) {
        self.on_raw_event(kind, json);
    }
}

/// Slot contents: zero or more Rust + C-ABI registrations (each with its
/// own kind filter), a monotonic id allocator, and the C-ABI fan-out
/// channel sender.
pub struct RawObserverInner {
    rust: Vec<Arc<RawRustObserverEntry>>,
    c_abi: Vec<Arc<RawCObserverEntry>>,
    next_id: u64,
    /// Sender half of the bounded C-ABI fan-out channel. Dropping the whole
    /// `RawObserverInner` drops this sender, ending the drain thread.
    c_fanout_tx: SyncSender<CRawFanoutEnvelope>,
}

impl RawObserverInner {
    fn new(c_fanout_tx: SyncSender<CRawFanoutEnvelope>) -> Self {
        Self {
            rust: Vec::new(),
            c_abi: Vec::new(),
            next_id: 1,
            c_fanout_tx,
        }
    }

    fn alloc_id(&mut self) -> RawEventObserverId {
        let id = RawEventObserverId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// `true` when no registration (Rust or C-ABI) would accept `kind`.
    /// Drives the ingest-side fast path so the verbatim-JSON serialization
    /// (and the duplicate Schnorr verify) are skipped entirely when nobody
    /// is listening for this kind.
    fn no_listener_for_kind(&self, kind: u32) -> bool {
        !self.rust.iter().any(|entry| entry.kinds.matches(kind))
            && !self
                .c_abi
                .iter()
                .any(|entry| entry.registration.kinds.matches(kind))
    }
}

/// Shared slot. The FFI surface holds one clone for registration; the
/// kernel holds another for invocation.
pub type RawEventObserverSlot = Arc<Mutex<RawObserverInner>>;

/// Invoke one decoupled C-ABI raw fan-out envelope. Runs on the per-slot
/// drain thread, never on the actor thread. Each callback is wrapped in
/// [`crate::ffi_guard::guard_ffi_callback`].
fn drain_c_raw_envelope(envelope: CRawFanoutEnvelope) {
    let ptr = envelope.payload.as_ptr();
    for entry in &envelope.registrations {
        let Some(_delivery) = entry.lifecycle.begin() else {
            continue;
        };
        let registration = &entry.registration;
        let _ = crate::ffi_guard::guard_ffi_callback("raw event observer", || {
            (registration.callback)(registration.context as *mut c_void, ptr);
        });
    }
}

/// Construct an empty slot, spawning its background C-ABI drain thread.
///
/// The drain thread lives for the life of the slot: it exits when the last
/// `Arc` to the `RawObserverInner` is dropped (which drops `c_fanout_tx`).
/// The slot's `Arc` is shared by `NmpApp` and the kernel actor and survives
/// `ActorCommand::Reset`, so the thread is spawned exactly once. The
/// `JoinHandle` is detached — there is no synchronous join point; the
/// dropped sender ends the thread cleanly on teardown.
///
/// Called once in `nmp_app_new`.
pub fn new_raw_event_observer_slot() -> RawEventObserverSlot {
    let (tx, rx) = sync_channel::<CRawFanoutEnvelope>(C_FANOUT_CHANNEL_BOUND);
    let _drain: JoinHandle<()> = std::thread::Builder::new()
        .name("nmp-raw-observer-drain".into())
        .spawn(move || {
            while let Ok(envelope) = rx.recv() {
                drain_c_raw_envelope(envelope);
            }
        })
        .expect("spawn raw event observer drain thread"); // doctrine-allow: D6 — runs once at process init (`nmp_app_new`); the slot return type is FFI-bound and cannot carry a `Result`. OS-level thread-spawn failure at startup is unrecoverable — the app cannot deliver raw events without this drain
    Arc::new(Mutex::new(RawObserverInner::new(tx)))
}

/// Register an in-process Rust observer with a kind filter. Returns an
/// opaque id the caller retains to unregister later.
pub fn register_rust_raw_observer(
    slot: &RawEventObserverSlot,
    kinds: KindFilter,
    observer: Arc<dyn RawEventObserver>,
) -> RawEventObserverId {
    let Ok(mut guard) = slot.lock() else {
        // Poisoned mutex — D6 silent fail.
        return RawEventObserverId(0);
    };
    let id = guard.alloc_id();
    guard.rust.push(Arc::new(RawRustObserverEntry {
        id,
        kinds,
        observer,
        lifecycle: Arc::new(RawObserverLifecycle::new()),
    }));
    id
}

/// Register a C-ABI observer. Returns an opaque id the caller retains to
/// unregister later.
pub fn register_c_raw_observer(
    slot: &RawEventObserverSlot,
    registration: RawEventObserverRegistration,
) -> RawEventObserverId {
    let Ok(mut guard) = slot.lock() else {
        return RawEventObserverId(0);
    };
    let id = guard.alloc_id();
    guard.c_abi.push(Arc::new(RawCObserverEntry {
        id,
        registration,
        lifecycle: Arc::new(RawObserverLifecycle::new()),
    }));
    id
}

/// Unregister by id (works for either Rust or C-ABI registrations).
/// Idempotent: unknown ids are silent no-ops.
///
/// For C-ABI registrations this is also a callback fence: queued envelopes
/// hold lifecycle-aware registration entries, so unregister marks the entry
/// inactive and waits for any in-flight callback to return before the call
/// completes. After this function returns, no callback for `id` can start.
pub fn unregister_raw_observer(slot: &RawEventObserverSlot, id: RawEventObserverId) {
    let mut lifecycles = Vec::new();
    if let Ok(mut guard) = slot.lock() {
        guard.rust.retain(|entry| {
            if entry.id == id {
                lifecycles.push(Arc::clone(&entry.lifecycle));
                false
            } else {
                true
            }
        });
        guard.c_abi.retain(|entry| {
            if entry.id == id {
                lifecycles.push(Arc::clone(&entry.lifecycle));
                false
            } else {
                true
            }
        });
    }
    for lifecycle in lifecycles {
        lifecycle.deactivate_and_wait();
    }
}

/// `true` when no registration would accept `kind`. The ingest tap calls
/// this first; on `true` it skips building / re-verifying / serializing the
/// event entirely (zero cost on the hot path when nobody taps that kind).
/// A poisoned mutex reports "no listener" (D6 — best-effort, never panics).
pub(crate) fn raw_observers_idle_for_kind(slot: &RawEventObserverSlot, kind: u32) -> bool {
    match slot.lock() {
        Ok(guard) => guard.no_listener_for_kind(kind),
        Err(_) => true,
    }
}

/// Fan one verbatim signed event out to every registration whose kind
/// filter matches `raw.kind`. Snapshot-and-release: the lock is held only
/// long enough to clone the matching registrations, so observers
/// re-registering inside their callback cannot deadlock.
///
/// **Rust** observers fire synchronously on the calling (actor) thread.
/// **C-ABI** observers are decoupled: the verbatim JSON is serialized once,
/// a `(snapshot, payload)` envelope is `try_send`-posted to the slot's
/// bounded channel, and the per-slot drain thread invokes the foreign
/// callbacks — `notify_raw_observers` never blocks on a callback's
/// duration. On channel overflow the envelope is dropped silently (D6
/// backpressure — library code performs no I/O). Serialization failure is a
/// D6 silent no-op.
pub(crate) fn notify_raw_observers(
    slot: &RawEventObserverSlot,
    raw: &RawEvent,
    source_relay_url: Option<&str>,
) {
    let kind = raw.kind;
    let (rust_snapshot, c_snapshot, c_fanout_tx) = {
        let Ok(guard) = slot.lock() else {
            return;
        };
        let rust: Vec<Arc<RawRustObserverEntry>> = guard
            .rust
            .iter()
            .filter(|entry| entry.kinds.matches(kind))
            .map(Arc::clone)
            .collect();
        let c_abi: Vec<Arc<RawCObserverEntry>> = guard
            .c_abi
            .iter()
            .filter(|entry| entry.registration.kinds.matches(kind))
            .map(Arc::clone)
            .collect();
        if rust.is_empty() && c_abi.is_empty() {
            return;
        }
        (rust, c_abi, guard.c_fanout_tx.clone())
    };

    // Serialize once. `RawEvent`'s struct field order is the NIP-01 order
    // `{id, pubkey, created_at, kind, tags, content, sig}` — the byte-
    // faithful verbatim signed event the consumer needs.
    let Ok(payload) = serde_json::to_string(raw) else {
        return;
    };

    for entry in &rust_snapshot {
        let Some(_delivery) = entry.lifecycle.begin() else {
            continue;
        };
        // D6: mirrors the in-process Rust-observer panic isolation in
        // `event_observer.rs`. A buggy `RawEventObserver` firing on the
        // actor thread must not unwind the kernel; wrap each call in
        // `catch_unwind` so one observer panicking does not stop its
        // siblings nor stall relay ingest. `AssertUnwindSafe` is sound:
        // the next iteration captures a fresh observer reference plus the
        // already-serialized `payload`. A swallowed panic still surfaces
        // via the default panic hook.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            entry
                .observer
                .on_raw_event_with_source(kind, &payload, source_relay_url);
        }));
    }

    if !c_snapshot.is_empty() {
        let Ok(cstr) = CString::new(payload) else {
            return;
        };
        let envelope = CRawFanoutEnvelope {
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
#[path = "raw_event_observer/tests.rs"]
mod tests;
