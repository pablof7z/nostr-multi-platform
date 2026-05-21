//! Raw signed-event observer slot.
//!
//! A generic, additive tap that delivers INBOUND verbatim-signed Nostr
//! events — the flat NIP-01 object `{id, pubkey, created_at, kind, tags,
//! content, sig}` *including the `sig`* — to a registered consumer, after
//! the kernel's existing Schnorr + id-hash gate has accepted the event.
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
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Bound on the per-slot C-ABI fan-out channel. See the equivalent constant
/// in `event_observer.rs` for the rationale.
const C_FANOUT_CHANNEL_BOUND: usize = 1024;

/// One unit of decoupled C-ABI raw fan-out work: the snapshot of matching C
/// registrations captured under the lock, plus the verbatim NIP-01 JSON
/// serialized once. The drain thread owns this and invokes each callback.
struct CRawFanoutEnvelope {
    registrations: Vec<RawEventObserverRegistration>,
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
    pub fn matches(&self, kind: u32) -> bool {
        self.0.is_empty() || self.0.contains(&kind)
    }

    /// `true` when no kinds are listed (match-everything).
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

/// In-process Rust observer. `Send + Sync` so it can live behind an `Arc`
/// shared between the actor thread and any registrant.
pub trait RawEventObserver: Send + Sync {
    /// Called once per accepted inbound event whose kind matches this
    /// observer's registered filter. `json` is the verbatim flat NIP-01
    /// signed-event JSON (`{id, pubkey, created_at, kind, tags, content,
    /// sig}`). Implementations must be cheap and must not panic — the call
    /// site is on the actor thread between relay frames.
    fn on_raw_event(&self, kind: u32, json: &str);
}

/// Slot contents: zero or more Rust + C-ABI registrations (each with its
/// own kind filter), a monotonic id allocator, and the C-ABI fan-out
/// channel sender.
pub struct RawObserverInner {
    rust: Vec<(RawEventObserverId, KindFilter, Arc<dyn RawEventObserver>)>,
    c_abi: Vec<(RawEventObserverId, RawEventObserverRegistration)>,
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
        !self.rust.iter().any(|(_, f, _)| f.matches(kind))
            && !self.c_abi.iter().any(|(_, r)| r.kinds.matches(kind))
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
    for registration in &envelope.registrations {
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
    let mut guard = match slot.lock() {
        Ok(g) => g,
        // Poisoned mutex — D6 silent fail.
        Err(_) => return RawEventObserverId(0),
    };
    let id = guard.alloc_id();
    guard.rust.push((id, kinds, observer));
    id
}

/// Register a C-ABI observer. Returns an opaque id the caller retains to
/// unregister later.
pub fn register_c_raw_observer(
    slot: &RawEventObserverSlot,
    registration: RawEventObserverRegistration,
) -> RawEventObserverId {
    let mut guard = match slot.lock() {
        Ok(g) => g,
        Err(_) => return RawEventObserverId(0),
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
/// once. The foreign caller's contract is unchanged — do not free the
/// registration's `context` pointer until you have fenced against any
/// in-flight callback (the decoupling only widens that pre-existing
/// window by the drain latency).
pub fn unregister_raw_observer(slot: &RawEventObserverSlot, id: RawEventObserverId) {
    if let Ok(mut guard) = slot.lock() {
        guard.rust.retain(|(rid, _, _)| *rid != id);
        guard.c_abi.retain(|(rid, _)| *rid != id);
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
pub fn notify_raw_observers(slot: &RawEventObserverSlot, raw: &RawEvent) {
    let kind = raw.kind;
    let (rust_snapshot, c_snapshot, c_fanout_tx) = {
        let Ok(guard) = slot.lock() else {
            return;
        };
        let rust: Vec<Arc<dyn RawEventObserver>> = guard
            .rust
            .iter()
            .filter(|(_, f, _)| f.matches(kind))
            .map(|(_, _, o)| Arc::clone(o))
            .collect();
        let c_abi: Vec<RawEventObserverRegistration> = guard
            .c_abi
            .iter()
            .filter(|(_, r)| r.kinds.matches(kind))
            .map(|(_, r)| r.clone())
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

    for observer in &rust_snapshot {
        observer.on_raw_event(kind, &payload);
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
    use std::time::{Duration, Instant};

    static C_CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_KIND: AtomicU32 = AtomicU32::new(0);
    static SERIAL: Mutex<()> = Mutex::new(());

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
        notify_raw_observers(&slot, &raw("aa", 1));
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
        notify_raw_observers(&slot, &raw("k1", 1)); // filtered out
        notify_raw_observers(&slot, &raw("k445", 445)); // delivered
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
        notify_raw_observers(&slot, &raw("nope", 1)); // filtered
        notify_raw_observers(&slot, &raw("yes", 1059)); // delivered
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
        notify_raw_observers(&slot, &raw("slow", 1));
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
    fn unregister_stops_callbacks() {
        let _g = SERIAL.lock().unwrap();
        let slot = new_raw_event_observer_slot();
        let obs = Arc::new(CapturingObserver(Mutex::new(Vec::new())));
        let id = register_rust_raw_observer(&slot, KindFilter::default(), obs.clone());
        notify_raw_observers(&slot, &raw("a", 1));
        unregister_raw_observer(&slot, id);
        notify_raw_observers(&slot, &raw("b", 1));
        assert_eq!(obs.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn empty_slot_is_silent() {
        let _g = SERIAL.lock().unwrap();
        let slot = new_raw_event_observer_slot();
        notify_raw_observers(&slot, &raw("a", 1)); // no panic, no-op
    }
}
