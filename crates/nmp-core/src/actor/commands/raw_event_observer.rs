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
//!   string (CString conversion failure), or panicking observer are silent
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
        crate::ffi_guard::guard_ffi_callback("raw event observer", || {
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
    let Ok(mut guard) = slot.lock() else { return RawEventObserverId(0); };
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
pub fn raw_observers_idle_for_kind(slot: &RawEventObserverSlot, kind: u32) -> bool {
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
pub fn notify_raw_observers(
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
                .on_raw_event_with_source(kind, &payload, source_relay_url)
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
mod tests {
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
}
