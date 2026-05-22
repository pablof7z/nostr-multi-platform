//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle wrappers + shared
//! argument helpers; `identity` carries the T66a identity / multi-account /
//! relay-edit wrappers; `publish` carries the publish-handle entry points
//! (signed/unsigned event publish, retry, cancel) — split out of `identity`
//! per AGENTS.md "co-locate by owner, not by role"; `timeline` carries the
//! open/close + profile claim/release wrappers; `testing` carries the
//! cfg-gated injectors (split to keep each file under the 300-LOC soft cap).

mod action;
mod capability;
mod event_observer;
mod identity;
mod lifecycle;
mod publish;
mod raw_event_tap;
mod snapshot;
mod timeline;
// D0: NIP-47 NWC is an app noun — the `nmp_app_wallet_*` FFI symbols are
// gated behind the `wallet` Cargo feature.
#[cfg(feature = "wallet")]
mod wallet;

#[cfg(any(test, feature = "test-support"))]
mod testing;

// Re-exported so the crate-level test-support facade (`lib.rs`) can reach
// these by the `ffi::` path, mirroring the other FFI entry points. The
// symbols stay `#[no_mangle] extern "C"` in `capability` so the Swift/C ABI
// is unaffected; the `pub use` itself is only consumed under the
// test-support gate, hence the matching `cfg`.
#[cfg(any(test, feature = "test-support"))]
pub use capability::{
    nmp_app_dispatch_capability, nmp_app_free_string, nmp_app_set_capability_callback,
};

// M6 — the single namespace-keyed action-dispatch entry point. Re-exported
// through the test-support facade so integration tests can call it through
// the rlib without an `extern "C"` block. The symbol stays `#[no_mangle]
// extern "C"` in `action`; the `pub use` is only consumed under the
// test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use action::{nmp_app_ack_action_stage, nmp_app_dispatch_action};

// Action-result observer registration — the push-side output seam. Re-exported
// through the test-support facade so integration tests can register an observer
// through the rlib without an `extern "C"` block. The symbol stays
// `#[no_mangle] extern "C"` in `action`; `allow(unused)`: the in-crate
// `ffi::action::tests` reach it by module path, so this facade re-export is
// used only by out-of-crate integration tests.
#[cfg(any(test, feature = "test-support"))]
#[allow(unused_imports)]
pub use action::nmp_app_register_action_result_observer;

// Host-extensible snapshot output — the `nmp_app_register_snapshot_projection`
// registration entry point. Re-exported through the test-support facade so
// integration tests can call it through the rlib without an `extern "C"`
// block. The symbol stays `#[no_mangle] extern "C"` in `snapshot`; the
// `pub use` is only consumed under the test-support gate. `allow(unused)`:
// the in-crate `ffi::snapshot::tests` reach the symbol by its module path,
// so the facade re-export is used only by out-of-crate integration tests.
#[cfg(any(test, feature = "test-support"))]
#[allow(unused_imports)]
pub use snapshot::nmp_app_register_snapshot_projection;

// T118 / G3 — lifecycle FFI exposed through the test-support facade so
// integration tests (`nmp-testing/tests/lifecycle_ffi_*`) can drive
// scenePhase transitions and assert on the observer callback. The Swift
// shell consumes the same `#[no_mangle] extern "C"` symbols directly via
// the static lib — the `pub use` only affects Rust-side reach.
#[cfg(any(test, feature = "test-support"))]
// `nmp_app_is_alive` is reached by the in-crate `ffi::lifecycle::tests`
// module by its `super::` path, so the facade re-export is only consumed by
// out-of-crate integration tests / test-support clients — same pattern as
// `nmp_app_register_action_result_observer` above. The `allow(unused)` keeps
// `cargo test -p nmp-core --lib` clean.
#[allow(unused_imports)]
pub use lifecycle::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_set_lifecycle_callback,
};

// T146 — kernel event observer FFI exposed through the test-support facade
// so integration tests in `nmp-testing` (and the in-tree FFI smoke in
// `event_observer.rs`) can register callbacks. Swift / Kotlin shells
// consume the same `#[no_mangle] extern "C"` symbols directly via the
// static lib.
#[cfg(any(test, feature = "test-support"))]
pub use event_observer::{nmp_app_register_event_observer, nmp_app_unregister_event_observer};

// Raw signed-event tap FFI exposed through the test-support facade so
// integration tests (and the in-tree smoke in `raw_event_tap.rs`) can
// register verbatim-signed-event callbacks. Swift / Kotlin shells consume
// the same `#[no_mangle] extern "C"` symbols directly via the static lib.
#[cfg(any(test, feature = "test-support"))]
pub use raw_event_tap::{
    nmp_app_register_raw_event_observer, nmp_app_unregister_raw_event_observer,
};

// Re-exported so `crate::ffi::nmp_app_inject_*` stays byte-stable for the
// test-support facade in `lib.rs`. The symbols stay `#[no_mangle] extern "C"`
// in `testing`; the `pub use` is only consumed under the test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use testing::{nmp_app_inject_pre_verified_events, nmp_app_inject_signed_events};

// Re-exported so `crate::ffi::nmp_app_{open_author,open_thread,...}` stays
// byte-stable for the test-support facade in `lib.rs`. The symbols stay
// `#[no_mangle] extern "C"` in `timeline`; the `pub use` is only consumed
// under the test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use timeline::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_close_thread, nmp_app_open_author,
    nmp_app_open_firehose_tag, nmp_app_open_thread, nmp_app_open_uri, nmp_app_release_profile,
};

// test-support: expose identity / relay-edit FFI entry-points so
// integration tests (and chirp-repl, which depends on nmp-core with the
// test-support feature) can call them through the rlib without extern "C"
// blocks. The symbols remain `#[no_mangle] extern "C"` in `identity`; this
// `pub use` is only consumed under the test/test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use identity::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_open_timeline, nmp_app_remove_relay,
    nmp_app_signin_nsec,
};

// test-support: expose the publish-lifecycle control-plane FFI entry-points
// (retry/cancel). The one-door-per-capability rule deleted the bespoke
// event-producing siblings `nmp_app_publish_signed_event` /
// `nmp_app_publish_signed_event_to` / `nmp_app_publish_unsigned_event` —
// those are the deleted door. Retry/cancel address a publish *handle* (not
// an event) and have no `dispatch_action` equivalent, so they stay on
// these dedicated symbols (the D11 lint whitelists them).
#[cfg(any(test, feature = "test-support"))]
pub use publish::{nmp_app_cancel_publish, nmp_app_retry_publish};

// android-ffi: expose all FFI entry-points via Rust paths so nmp-android-ffi
// can call them through the rlib. These re-exports are the ONLY thing that
// makes rustc include the symbol bodies in CGU files for the cdylib.
#[cfg(feature = "android-ffi")]
pub use identity::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_open_timeline, nmp_app_remove_account,
    nmp_app_remove_relay, nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
};
// android-ffi: publish-lifecycle control-plane FFI (retry/cancel). The
// one-door-per-capability rule deleted the bespoke event-producing siblings
// from this module; what remains is the narrow control surface the action
// seam does not carry.
#[cfg(feature = "android-ffi")]
pub use publish::{nmp_app_cancel_publish, nmp_app_retry_publish};
// T118 / G3 — android-ffi must also reach the lifecycle symbols; without this
// re-export rustc doesn't pull the symbol bodies into the cdylib CGU and the
// Android JNI shim can't link.
#[cfg(feature = "android-ffi")]
pub use capability::{
    nmp_app_dispatch_capability, nmp_app_free_string, nmp_app_set_capability_callback,
};
// M6 — action-dispatch entry point + the action-result observer registration
// seam, reachable via the Rust path so the Android JNI shim pulls the symbol
// bodies into the cdylib CGU. Each symbol is `#[no_mangle] extern "C"` in
// `action`; without this re-export rustc omits its body from the cdylib CGU
// and an Android link step against it fails (the `cargo check (android-ffi)`
// CI job never links, so it does not catch this). `allow(unused_imports)`:
// the re-export exists only to force the symbol body into the cdylib CGU —
// no Rust caller names it by this path.
//
// ADR-0027 final stage: the closure-based dual seam
// (`nmp_app_register_action_executor` / `nmp_app_register_action_module`) was
// deleted; the typed `register_action::<M>()` Rust seam is the sole host
// registration path. There is no useful C-ABI shape for the typed seam —
// `M::Action` and `ActorCommand` have no stable C representation.
#[cfg(feature = "android-ffi")]
#[allow(unused_imports)]
pub use action::{nmp_app_dispatch_action, nmp_app_register_action_result_observer};
// Host-extensible snapshot output — registration entry point reachable via
// the Rust path so the Android JNI shim pulls the symbol body into the
// cdylib CGU.
#[cfg(feature = "android-ffi")]
pub use lifecycle::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_set_lifecycle_callback,
};
#[cfg(feature = "android-ffi")]
pub use snapshot::nmp_app_register_snapshot_projection;
// T146 — kernel event observer FFI symbols reachable via Rust paths so the
// Android JNI shim can pull the symbol bodies into the cdylib CGU.
#[cfg(feature = "android-ffi")]
pub use event_observer::{nmp_app_register_event_observer, nmp_app_unregister_event_observer};
#[cfg(feature = "android-ffi")]
pub use raw_event_tap::{
    nmp_app_register_raw_event_observer, nmp_app_unregister_raw_event_observer,
};
#[cfg(feature = "android-ffi")]
pub use timeline::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_close_thread, nmp_app_open_author,
    nmp_app_open_firehose_tag, nmp_app_open_thread, nmp_app_open_uri, nmp_app_release_profile,
};
#[cfg(all(feature = "android-ffi", feature = "wallet"))]
pub use wallet::{nmp_app_wallet_connect, nmp_app_wallet_disconnect, nmp_app_wallet_pay_invoice};

use crate::actor::{
    new_event_observer_slot, new_lifecycle_observer_slot, new_raw_event_observer_slot,
    register_rust_observer, register_rust_raw_observer, run_actor_with_observers,
    unregister_observer, unregister_raw_observer, ActorCommand, KernelEventObserver,
    KernelEventObserverId, KernelEventObserverSlot, KindFilter, LifecycleObserverSlot,
    RawEventObserver, RawEventObserverId, RawEventObserverSlot,
};
use crate::capability_socket::{new_capability_callback_slot, CapabilityCallbackSlot};
use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use zeroize::Zeroizing;

type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);

#[derive(Clone, Copy)]
struct UpdateCallbackRegistration {
    context: usize,
    callback: UpdateCallback,
}

pub struct NmpApp {
    tx: Sender<ActorCommand>,
    update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>>,
    capability_callback: CapabilityCallbackSlot,
    /// T118 / G3 — lifecycle observer slot. Shared `Arc` with the actor
    /// thread: registrations through [`lifecycle::nmp_app_set_lifecycle_callback`]
    /// are visible to the actor without crossing the FFI on each event.
    lifecycle_observer: LifecycleObserverSlot,
    /// T146 — kernel event observer slot. Shared `Arc` with the actor
    /// thread (and thus the kernel, which `crate::actor::run_actor_with_
    /// observers` binds onto the kernel via `set_event_observers_handle`).
    /// Per-app crates (e.g. a per-app crate) reach this slot through
    /// [`NmpApp::register_event_observer`] /
    /// [`NmpApp::unregister_event_observer`]; the C-ABI variant goes
    /// through `ffi::event_observer::nmp_app_register_event_observer`. Both
    /// paths mutate the same `Mutex<…>` the actor reads.
    event_observers: KernelEventObserverSlot,
    /// Raw signed-event tap slot. Shared `Arc` with the actor thread (and
    /// thus the kernel, which `run_actor_with_observers` binds via
    /// `set_raw_event_observers_handle`). Per-app crates reach this through
    /// [`NmpApp::register_raw_event_observer`] /
    /// [`NmpApp::unregister_raw_event_observer`]; the C-ABI variant goes
    /// through `ffi::raw_event_tap::nmp_app_register_raw_event_observer`.
    /// Both paths mutate the same `Mutex<…>` the actor reads. Delivers the
    /// verbatim flat NIP-01 signed event (`sig` included), kind-filtered.
    raw_event_observers: RawEventObserverSlot,
    /// Previously-installed NIP-17 DM-inbox raw-event observer id, for the
    /// idempotent re-invoke contract on the per-app crate's
    /// `register_dm_inbox` entry point. The per-app crate (e.g. `nmp-app-chirp`)
    /// swaps the slot atomically via [`Self::swap_nip17_dm_inbox_observer`]:
    /// the previous id is taken out, the new observer is registered, and the
    /// new id is stored back — so a re-invoke unregisters the prior observer
    /// before installing the new one, instead of stacking a fresh observer on
    /// every sign-in / account-switch cycle.
    ///
    /// Protocol-named (not chirp-named) because it tracks a NIP-17 surface;
    /// any host wiring a single DM-inbox per app shares this contract. A
    /// multi-inbox host would need a handle-returning variant instead.
    nip17_dm_inbox_observer_id: Arc<Mutex<Option<RawEventObserverId>>>,
    /// Singleton kernel-event observer-id slot used by per-app crates that
    /// register exactly one auxiliary `KernelEventObserver` per app and want
    /// the registration to be idempotent across re-invokes — see
    /// [`Self::swap_singleton_event_observer`]. The per-app crate swaps the
    /// slot atomically: the previous id is taken out, the new observer is
    /// registered, and the new id is stored back — so a re-invoke
    /// unregisters the prior observer before installing the new one,
    /// instead of stacking a fresh observer on every re-entry.
    ///
    /// Substrate-generic (kernel-level): the slot holds a bare
    /// [`KernelEventObserverId`]; the per-app crate decides what protocol
    /// surface uses it (D0 — the kernel never names the app noun). The
    /// first internal consumer is `nmp-app-chirp`'s per-app group-chat
    /// registration. A host that wants to keep N projections live in
    /// parallel still needs a handle-returning variant.
    singleton_event_observer_id: Arc<Mutex<Option<KernelEventObserverId>>>,
    /// Shared relay-edit rows handle. Cloned to the actor thread and bound
    /// onto the kernel so external Rust callers (e.g. per-app crates) can read
    /// the user's current relay list without crossing FFI.
    ///
    /// The slot is a typed [`crate::kernel::RelayEditRowsSlot`]
    /// (`Arc<Mutex<RelayEditRowList>>`) — D14 forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp` and the typed wrapper makes
    /// the slot's purpose visible at the declaration site.
    relay_edit_rows: crate::kernel::RelayEditRowsSlot,
    /// Active local (nsec-backed) secret key in bech32 form (`nsec1…`). The
    /// actor thread writes this after every identity mutation that changes
    /// the active local key (create, sign-in, switch, remove). Remote-signer
    /// accounts leave this `None`. Per-app crates (e.g. `nmp-app-chirp`
    /// Marmot) read it via [`NmpApp::marmot_local_nsec`] so they can
    /// register a signer without Swift ever seeing the key.
    ///
    /// ADR-0025 exception: Marmot needs the raw nsec for MLS. NIP-17 DMs must
    /// NOT read this slot.
    ///
    /// Wrapped in [`Zeroizing`] so the bech32 secret is wiped from the heap
    /// when the slot is overwritten or the app drops — a plain `String` would
    /// leave the key recoverable in freed memory.
    marmot_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>>,
    /// Active account's local `nostr::Keys`, or `None` for a remote-signer
    /// (NIP-46 / bunker) account. The actor thread writes this after every
    /// identity mutation that changes the active local key (create, sign-in,
    /// switch, remove) — exactly parallel to `marmot_local_nsec`.
    ///
    /// This slot is the NIP-44 key seam for protocol-crate consumers that
    /// need the in-process keypair to seal / unseal gift-wraps (NIP-17 DM
    /// inbox decryption). It is DISTINCT from `marmot_local_nsec`: that field
    /// is the ADR-0025 bounded exception for MLS, and the ADR explicitly
    /// scopes the exception. A consumer without this exception reads
    /// THIS slot instead.
    ///
    /// `nostr::Keys` is `Clone` and zeroizes its own secret on drop, so no
    /// `Zeroizing` wrapper is needed here.
    nip17_local_keys: Arc<Mutex<Option<nostr::Keys>>>,
    /// FFI-supplied persistent storage directory for the LMDB `EventStore`
    /// backend. Set by [`nmp_app_set_storage_path`] before
    /// [`nmp_app_start`]. Shared `Arc` with the actor thread: the C-ABI
    /// setter writes through this clone, the actor reads its clone when it
    /// constructs the kernel (`run_actor_with_observers` →
    /// `Kernel::with_storage_path` → `build_event_store`).
    ///
    /// `None` (the default until a host calls the setter) keeps the
    /// in-memory store. The path is only honoured when the crate is built
    /// with `--features lmdb-backend`; otherwise it is inert.
    storage_path: Arc<Mutex<Option<String>>>,
    /// One-shot account-creation intent: when true, the app-level MLS
    /// composition layer should publish a key package after it registers the new
    /// local identity. Kept beside the app handle because `nmp-core` owns the
    /// single account-creation FFI verb while app crates own MLS details.
    ///
    /// A bare `AtomicBool` — this flag is only ever read/written through
    /// `&self` accessors on this struct and is never shared with the actor
    /// thread (unlike the `Arc<Mutex<…>>` observer/storage slots, no clone is
    /// handed to `run_actor_with_observers`). A `Mutex<bool>` would be the
    /// wrong primitive for a single-shot lock-free flag, and the `Arc` would
    /// be dead shared ownership nothing consumes.
    pending_mls_autopublish: AtomicBool,
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
    /// M6 — namespace-keyed action-dispatch registry backing
    /// [`action::nmp_app_dispatch_action`]. Holds only stateless ZST module
    /// adapters, so it is `Send + Sync` and is queried directly on the FFI
    /// thread (no actor round-trip): registered modules' `start` methods are
    /// pure validators. The `Kernel`-side wiring (execution + the durable
    /// action ledger) is the M6 follow-up; see
    /// [`crate::kernel::action_registry`].
    action_registry: crate::kernel::ActionRegistry,
    /// Host-extensible snapshot output registry — the output-side counterpart
    /// to `action_registry`. Shared `Arc<Mutex<…>>` with the actor thread
    /// (bound onto the kernel via `set_snapshot_projection_handle`): a host
    /// registers a projection closure through
    /// [`Self::register_snapshot_projection`] / the C-ABI
    /// `nmp_app_register_snapshot_projection`, and the kernel runs every
    /// registered closure in `make_update`, appending the result to
    /// `KernelSnapshot::projections`. Unlike `action_registry`, this is NOT
    /// queried on the FFI thread — it fires from inside the actor tick, hence
    /// the shared-`Arc` slot rather than a plain owned field.
    snapshot_projections: crate::kernel::SnapshotProjectionSlot,
    /// G-S4 — straddle counter for the actor command channel depth. The
    /// command channel is an unbounded `std::sync::mpsc::channel()` whose
    /// `Receiver` has no `len()`, so depth is observed indirectly: `send_cmd`
    /// (the sole funnel for FFI command sends) does `fetch_add(1)` before the
    /// `send`, and the actor does `fetch_sub(1)` per command it dequeues. The
    /// matching `Arc` clone is bound onto the kernel via
    /// `set_queue_depth_handle` so `make_update` surfaces the value as
    /// `Metrics::actor_queue_depth`. `Relaxed` ordering throughout — this is an
    /// approximate observability counter, not a synchronization primitive.
    ///
    /// Note: external sends through [`Self::actor_sender`] (used by
    /// `nmp-signer-broker`) bypass this counter; the depth is therefore a
    /// lower bound when a broker is wired. That is acceptable for the
    /// backpressure gate, which watches for *buildup*, not exact occupancy.
    queue_depth: Arc<AtomicU64>,
    /// NIP-47 wallet double-tap guard: bolt11 strings the FFI surface has
    /// already accepted for `pay_invoice` but for which the kind:23195
    /// response (or a timeout) has not yet cleared. Keyed by the full bolt11
    /// string — a same-invoice retap maps to the same key, and the FFI
    /// short-circuits before constructing a second
    /// `ActorCommand::WalletPayInvoice`.
    ///
    /// Lives entirely in the FFI layer (no actor coupling): expiry is wall-
    /// clock based — entries older than [`wallet::INFLIGHT_BOLT11_TTL`] are
    /// swept at every `nmp_app_wallet_pay_invoice` call, so a legitimate retry
    /// after the TTL passes through. The TTL is sized for "the NWC response
    /// is in flight" — long enough to absorb relay round-trip jitter, short
    /// enough that a wallet that never responds does not block the user
    /// forever.
    ///
    /// D14: `Mutex<HashMap<…>>` is NOT the banned `Arc<Mutex<Vec<…>>>` shape
    /// the rule disciplines (a `HashMap` is not a `Vec`, and the slot is not
    /// shared with the actor — no `Arc`). The simpler primitive is correct
    /// here: nothing on the actor side reads or writes this slot, so the
    /// `Arc` clone would be dead shared ownership.
    #[cfg(feature = "wallet")]
    inflight_bolt11: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// Generic dispatch idempotency guard: dedup-keys for
    /// [`action::nmp_app_dispatch_action`] calls accepted by the registry but
    /// whose action-result has not yet cleared. Keyed by a stable 64-bit
    /// FNV-1a hash of `(namespace, action_json)` (see
    /// [`crate::stable_hash::stable_hash64`]) — a same-action retap inside
    /// the TTL window maps to the same key, and the FFI short-circuits before
    /// the second `start()` + executor pass enqueues a duplicate
    /// `ActorCommand`. The stored value is `(when_first_seen,
    /// original_correlation_id)`: a dedup hit returns the original id in the
    /// `{"correlation_id":...}` envelope so the host's spinner stays bound to
    /// the first dispatch (mirrors the "id stays bound" semantic of the
    /// `executor_failure_returns_correlation_id_and_enqueues_
    /// failed_terminal` test).
    ///
    /// Mirrors the [`inflight_bolt11`] pattern but for ALL action namespaces,
    /// not just `pay_invoice`. The primary motivator is the NIP-17 DM send
    /// path: a rapid re-tap on Send before the gift-wrap fan-out completes
    /// would otherwise mint a second batch of kind:1059 envelopes that are
    /// indistinguishable to recipients (no on-the-wire dedup is possible).
    /// The guard is generic by intent — every namespace the host dispatches
    /// shares the same 30-second wall-clock window.
    ///
    /// Lives entirely in the FFI layer (no actor coupling): expiry is
    /// wall-clock based — entries older than
    /// [`action::INFLIGHT_DISPATCH_TTL`] are swept at every
    /// `dispatch_action_json` call, so a legitimate retry after the TTL
    /// passes through.
    ///
    /// D14: `Mutex<HashMap<…>>` is NOT the banned `Arc<Mutex<Vec<…>>>` shape
    /// — same reasoning as `inflight_bolt11` above.
    inflight_dispatches: Mutex<std::collections::HashMap<u64, (std::time::Instant, String)>>,
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut callback) = self.update_callback.lock() {
            *callback = None;
        }
        // Route through `send_cmd` so the G-S4 queue-depth counter stays
        // consistent: the actor decrements it as it dequeues `Shutdown`.
        self.send_cmd(ActorCommand::Shutdown);
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        if let Ok(mut listener) = self.update_listener.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>> =
        Arc::new(Mutex::new(None));
    let listener_callback = Arc::clone(&update_callback);
    // T118 / G3 — shared lifecycle observer slot. The FFI side
    // (`nmp_app_set_lifecycle_callback`) writes registrations through one
    // clone; the actor thread reads through the other when handling
    // `ActorCommand::LifecycleEvent`. Both see the same `Mutex<Option<...>>`.
    let lifecycle_observer = new_lifecycle_observer_slot();
    let actor_lifecycle_observer = Arc::clone(&lifecycle_observer);
    // T146 — shared kernel event observer slot. Same pattern as the
    // lifecycle slot: the `NmpApp` keeps one clone (used by Rust + C-ABI
    // registration entry points), the actor thread carries another for the
    // kernel's fan-out path (`set_event_observers_handle`). Registrations
    // mutate the inner `Mutex` visible to both sides.
    let event_observers = new_event_observer_slot();
    let actor_event_observers = Arc::clone(&event_observers);
    // Raw signed-event tap slot. Same shared-`Arc` pattern: the `NmpApp`
    // keeps one clone (Rust + C-ABI registration entry points), the actor
    // thread carries another for the kernel's tap path
    // (`set_raw_event_observers_handle`).
    let raw_event_observers = new_raw_event_observer_slot();
    let actor_raw_event_observers = Arc::clone(&raw_event_observers);
    // Per-app idempotency slots — track the previously-installed observer id
    // for single-instance auxiliary observer registrations a per-app crate
    // (e.g. `nmp-app-chirp`) wires through `swap_nip17_dm_inbox_observer` /
    // `swap_singleton_event_observer`. NOT shared with the actor thread —
    // the actor never reads these; only the FFI side calls the swap
    // accessors. Owned by `NmpApp`, dropped with it (so the slot dies with
    // the app — no global aliasing across `nmp_app_free`).
    let nip17_dm_inbox_observer_id: Arc<Mutex<Option<RawEventObserverId>>> =
        Arc::new(Mutex::new(None));
    let singleton_event_observer_id: Arc<Mutex<Option<KernelEventObserverId>>> =
        Arc::new(Mutex::new(None));
    // Host-extensible snapshot output slot. Same shared-`Arc` pattern: the
    // `NmpApp` keeps one clone (Rust + C-ABI registration entry points), the
    // actor thread carries another and binds it onto the kernel
    // (`set_snapshot_projection_handle`). Registrations mutate the inner
    // `Mutex<SnapshotRegistry>` visible to both sides.
    let snapshot_projections = crate::kernel::new_snapshot_projection_slot();
    let actor_snapshot_projections = Arc::clone(&snapshot_projections);
    // D0: NIP-47 NWC is an app noun. The shared wallet-status slot — one `Arc`
    // clone goes to the actor's `WalletRuntime` (the sole writer, D4), the
    // other is captured below by the `"wallet"` snapshot-projection closure so
    // wallet state reaches the host through `projections["wallet"]` instead of
    // a baked-in `KernelSnapshot` field. The wallet projection is registered
    // unconditionally (under the feature gate) right after the actor spawns:
    // the projection contributes JSON `null` until a wallet connects, which
    // preserves the "key present, value null when disconnected" semantic the
    // social shells already decode.
    #[cfg(feature = "wallet")]
    let wallet_status = crate::actor::new_wallet_status_slot();
    #[cfg(feature = "wallet")]
    let actor_wallet_status = Arc::clone(&wallet_status);
    // D0: NIP-46 remote signing is an app noun. The shared bunker-handshake
    // slot is handed to the actor: `run_actor_with_observers` both gives one
    // `Arc` clone to the actor's `IdentityRuntime` (the sole writer, D4) and
    // registers the built-in `"bunker_handshake"` snapshot-projection closure
    // that reads the other clone. Handshake state therefore reaches the host
    // through `projections["bunker_handshake"]` instead of a baked-in
    // `KernelSnapshot` field — and every actor consumer (FFI or test) gets the
    // projection without a separate FFI registration step.
    let actor_bunker_handshake = crate::actor::new_bunker_handshake_slot();
    // Shared relay-edit rows handle. Cloned to the actor thread and bound
    // onto the kernel so external Rust callers can read the user's current
    // relay list without crossing FFI.
    //
    // Typed slot constructor — see `kernel/relay_projection.rs`.
    let relay_edit_rows: crate::kernel::RelayEditRowsSlot =
        crate::kernel::new_relay_edit_rows_slot();
    let actor_relay_edit_rows = Arc::clone(&relay_edit_rows);
    // Active local (nsec) key slot. The actor updates this after every
    // identity mutation; per-app crates read via NmpApp::marmot_local_nsec.
    let marmot_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>> = Arc::new(Mutex::new(None));
    let actor_marmot_local_nsec = Arc::clone(&marmot_local_nsec);
    // Active local `nostr::Keys` slot — the NIP-44 key seam for non-ADR-0025
    // protocol consumers (NIP-17 DM inbox decryption). Same shared-`Arc`
    // pattern as `marmot_local_nsec`: the actor updates it on every identity
    // mutation; per-app crates read via `NmpApp::nip17_local_keys`.
    let nip17_local_keys: Arc<Mutex<Option<nostr::Keys>>> = Arc::new(Mutex::new(None));
    let actor_nip17_local_keys = Arc::clone(&nip17_local_keys);
    // Shared capability callback slot. FFI registration writes through the
    // app clone; the actor reads through its clone when issuing keyring
    // requests during start/sign-in/create/switch/remove.
    let capability_callback = new_capability_callback_slot();
    let actor_capability_callback = Arc::clone(&capability_callback);
    // FFI-supplied LMDB storage path slot. `nmp_app_set_storage_path`
    // writes through the `NmpApp`'s clone before `nmp_app_start`; the actor
    // reads through this clone when it builds the kernel. Default `None`
    // → in-memory store.
    let storage_path: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let actor_storage_path = Arc::clone(&storage_path);
    // One-shot MLS-autopublish intent flag. Not shared with the actor thread,
    // so a bare `AtomicBool` — no `Arc`, no `Mutex` — is the right primitive.
    let pending_mls_autopublish = AtomicBool::new(false);
    // G-S4 — actor command-channel depth straddle counter. The `NmpApp` keeps
    // one `Arc` clone (incremented by `send_cmd` before every channel send);
    // the actor carries the other (decremented per command dequeued) and binds
    // it onto the kernel so `make_update` reads it. See the `queue_depth` field
    // doc on `NmpApp` for the full contract.
    let queue_depth: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let actor_queue_depth = Arc::clone(&queue_depth);
    // Clone so we can report actor death through the same listener pipe.
    // The actor `move`s its own `update_tx` into `run_actor_with_observers`;
    // this clone is the supervisor's last live handle once that one is
    // dropped — it MUST outlive the inner closure so the panic frame can
    // still be delivered after the actor's own sender is gone.
    let update_tx_panic = update_tx.clone();
    // Self-feedback sender for the actor — a clone of the command sender
    // that the host also keeps (`command_tx` above). Background workers
    // spawned from dispatch arms (currently the LNURL-pay round-trip the
    // `FetchLnurlInvoice` arm starts) use this clone to send follow-up
    // `ActorCommand`s back into the loop without crossing FFI.
    //
    // G-S4 caveat: sends through this clone bypass the `queue_depth`
    // straddle counter (the only incrementing path is `NmpApp::send_cmd`).
    // The `actor_queue_depth` snapshot metric is therefore a lower bound
    // for self-feedback traffic — acceptable for a backpressure gate that
    // watches for buildup, matches the existing `actor_sender()` caveat.
    let actor_command_tx_self = command_tx.clone();
    let actor = thread::spawn(move || {
        // D7 (actor-death visibility): the actor thread owns the kernel loop.
        // If it panics, `send_cmd` would otherwise silently drop every
        // subsequent command (the channel closes with no signal). Catch the
        // unwind here and emit one envelope-conforming `Panic` frame on the
        // update channel *before* this thread (and `update_tx`) is dropped,
        // so the host receives a terminal, decodable signal.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_actor_with_observers(
                command_rx,
                actor_command_tx_self,
                update_tx,
                actor_lifecycle_observer,
                actor_event_observers,
                actor_raw_event_observers,
                actor_snapshot_projections,
                // D0: NIP-47 NWC is an app noun — the wallet-status slot the
                // actor's `WalletRuntime` writes; the `"wallet"` projection
                // (registered below) reads the matching clone.
                #[cfg(feature = "wallet")]
                actor_wallet_status,
                // D0: NIP-46 remote signing is an app noun — the
                // bunker-handshake slot the actor's `IdentityRuntime` writes;
                // the `"bunker_handshake"` projection (registered below) reads
                // the matching clone.
                actor_bunker_handshake,
                actor_relay_edit_rows,
                actor_marmot_local_nsec,
                actor_nip17_local_keys,
                actor_capability_callback,
                actor_storage_path,
                // G-S4 — the actor's clone of the command-channel depth
                // counter. Decremented per dequeued command; bound onto the
                // kernel for the `actor_queue_depth` snapshot field.
                actor_queue_depth,
            );
        }));
        if let Err(e) = result {
            // Best-effort downcast of the panic payload — see
            // `update_envelope::panic_message`. D6: `panic_message` and
            // `wrap_panic` are both infallible (placeholder / constant-frame
            // fallbacks), so building the death signal cannot itself panic.
            // The resulting `{"t":"panic","v":{"msg":…}}` decodes cleanly
            // into `UpdateEnvelope::Panic` — unlike the previous ad-hoc
            // `{"t":"panic","m":…}` string, which did not match the
            // envelope's tag/content schema and failed host decode.
            let msg = crate::update_envelope::panic_message(&*e);
            let frame = crate::update_envelope::wrap_panic(format!("actor thread died: {msg}"));
            let _ = update_tx_panic.send(frame);
        }
    });
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            let Ok(payload) = CString::new(update) else {
                continue;
            };
            let callback = listener_callback.lock().ok().and_then(|guard| *guard);
            if let Some(registration) = callback {
                // UB guard: the foreign update callback may panic / raise.
                // This listener thread has no outer `catch_unwind` (unlike
                // the actor thread above), so an unguarded unwind here is
                // undefined behaviour across the C ABI boundary.
                crate::ffi_guard::guard_ffi_callback("update listener", || {
                    (registration.callback)(registration.context as *mut c_void, payload.as_ptr());
                });
            }
        }
    });

    let app = NmpApp {
        tx: command_tx,
        update_callback,
        capability_callback,
        lifecycle_observer,
        event_observers,
        raw_event_observers,
        nip17_dm_inbox_observer_id,
        singleton_event_observer_id,
        relay_edit_rows,
        marmot_local_nsec,
        nip17_local_keys,
        storage_path,
        pending_mls_autopublish,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
        // M6 — the action registry the kernel ships with: `PublishModule`
        // only. NIP-29 / NIP-59 modules are app nouns (D0) and are
        // registered by the app host against its own registry instance.
        action_registry: crate::kernel::default_registry(),
        // Host-extensible snapshot output: ships with the built-in `"wallet"`
        // projection (registered below when `feature = "wallet"`). A non-social
        // host registers its own projections via
        // `nmp_app_register_snapshot_projection` during init.
        snapshot_projections,
        // G-S4 — the `NmpApp`'s clone of the command-channel depth counter,
        // incremented by `send_cmd`. The actor holds the matching clone.
        queue_depth,
        // NIP-47 wallet `pay_invoice` double-tap guard. Empty at construction;
        // populated by `ffi::wallet::nmp_app_wallet_pay_invoice` on each
        // accepted invoice, swept on TTL expiry (no cross-thread coupling).
        #[cfg(feature = "wallet")]
        inflight_bolt11: Mutex::new(std::collections::HashMap::new()),
        // Generic dispatch idempotency guard. Empty at construction; populated
        // by `ffi::action::dispatch_action_json` on each accepted dispatch,
        // swept on TTL expiry (no cross-thread coupling).
        inflight_dispatches: Mutex::new(std::collections::HashMap::new()),
    };

    // D0 — first internal consumer of the snapshot-projection seam: register
    // the built-in `"wallet"` projection. NIP-47 NWC is an app noun, so wallet
    // state is NOT a typed `KernelSnapshot` field — it is projected under
    // `projections["wallet"]` exactly like a host-registered namespace. The
    // closure captures the shared `wallet_status` slot the actor's
    // `WalletRuntime` writes; it runs on every snapshot tick (D8: cheap,
    // non-blocking — a single lock-and-clone). When no wallet is connected the
    // slot holds `None` and the closure contributes JSON `null`, preserving the
    // "key present, value null when disconnected" semantic.
    #[cfg(feature = "wallet")]
    app.register_snapshot_projection("wallet", move || {
        match wallet_status.lock() {
            Ok(slot) => slot
                .as_ref()
                .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null),
            // D6: a poisoned wallet-status mutex collapses to `null` rather
            // than panicking inside the snapshot tick.
            Err(_) => serde_json::Value::Null,
        }
    });

    // D0 — the built-in `"bunker_handshake"` projection is registered inside
    // `run_actor_with_observers` (at the actor wiring site), not here: it
    // reads the actor-owned bunker-handshake slot, so every actor consumer
    // (FFI or test) gets the projection without a separate FFI step.

    Box::into_raw(Box::new(app))
}

impl NmpApp {
    /// Send a command to the actor thread.
    ///
    /// D6: a disconnected channel (actor thread panicked or exited) must
    /// degrade gracefully — never panic, never write to stderr from library
    /// code. The send is best-effort; the dropped command is the failure
    /// signal.
    ///
    /// D7 (actor-death visibility): if the actor thread panics, the
    /// supervisor closure in `nmp_app_new` emits one
    /// `UpdateEnvelope::Panic` frame on the update channel before the channel
    /// closes — see [`crate::update_envelope`]'s actor-death contract. So a
    /// dropped command here is no longer *silent*: the host has already
    /// received (or will receive) the terminal panic frame and is expected
    /// to surface a fatal error rather than keep sending.
    pub(crate) fn send_cmd(&self, cmd: ActorCommand) {
        // G-S4 — straddle counter: increment before the send so the kernel
        // never observes a command "in flight" with a stale-low depth. The
        // actor decrements as it dequeues. `Relaxed` is sufficient — the value
        // is approximate observability, not a synchronization edge. If the
        // send fails (actor thread gone) the command is dropped and the
        // counter is left one high; that is harmless on a dead actor.
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(cmd);
    }

    /// Register a typed [`crate::substrate::ActionModule`] `M` against the
    /// app's action registry — ADR-0027's single-call typed seam, and the
    /// sole host action-registration path on master.
    ///
    /// `M::start` handles validation AND `M::execute` handles execution, both
    /// under the same typed namespace (`M::NAMESPACE`): there is no possible
    /// partial-registration gap (the pre-ADR-0027 dual `register_action_module`
    /// / `register_action_executor` closure seam has been deleted).
    ///
    /// Registration MUST happen during host init — before `nmp_app_start`
    /// and before any [`action::nmp_app_dispatch_action`] call — because it
    /// requires `&mut self`.
    pub fn register_action<M: crate::substrate::ActionModule + 'static>(&mut self) {
        self.action_registry.register::<M>();
    }

    /// Register a host-supplied snapshot projection — the output-side
    /// counterpart to [`Self::register_action_executor`].
    ///
    /// The closure runs on **every snapshot tick** (inside the actor's
    /// `make_update`) and its returned JSON value is appended to
    /// `KernelSnapshot::projections` under `key`. A marketplace app registers
    /// `"market.listings"`, a todo app registers `"todo.items"` — each gets
    /// its own snapshot namespace WITHOUT editing `nmp-core`'s sealed social
    /// `KernelSnapshot` fields.
    ///
    /// Unlike `register_action_executor`, this does NOT require `&mut self`:
    /// the registry lives behind a shared `Arc<Mutex<…>>` and the mutation is
    /// a lock-and-push. It is still intended as a host-init call.
    ///
    /// D8 — the closure runs on the actor thread inside the snapshot tick. It
    /// MUST be cheap and non-blocking (no I/O, no mutex waits): a blocking
    /// closure stalls every subsequent snapshot and freezes the host's
    /// update stream. A poisoned registry mutex is a silent no-op (D6).
    pub fn register_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> serde_json::Value + Send + Sync + 'static,
    ) {
        if let Ok(mut registry) = self.snapshot_projections.lock() {
            registry.register(key, f);
        }
    }

    /// Register a host-supplied action-result observer — the *push*
    /// counterpart to [`Self::register_snapshot_projection`]'s pull seam.
    ///
    /// After [`action::nmp_app_dispatch_action`] accepts an action and its
    /// executor returns `Ok`, the observer is handed a
    /// [`crate::substrate::ActionResult`] carrying the action's
    /// `correlation_id`. This is an "action accepted and enqueued" signal,
    /// not a completion carrier — for `nmp.publish` the actor still has to
    /// verify+publish after this fires; that outcome reaches the host via
    /// the snapshot-projection (pull) path.
    ///
    /// Like `register_snapshot_projection`, this does NOT require `&mut self`:
    /// the observer lives behind a shared `Arc<Mutex<…>>` slot inside the
    /// action registry, so a host may register it before or after
    /// `nmp_app_start`. A second registration replaces the first.
    pub fn register_action_result_observer(
        &self,
        f: impl Fn(crate::substrate::ActionResult) + Send + Sync + 'static,
    ) {
        self.action_registry.set_result_observer(f);
    }

    /// Test-only: run every registered snapshot projection directly against
    /// the app's shared registry, bypassing the actor/kernel tick. The
    /// end-to-end `make_update`-driven proof lives in the kernel test module
    /// (`kernel/snapshot_registry_tests.rs`); this helper lets the FFI
    /// registration tests assert the C-callback bridge in isolation.
    #[cfg(test)]
    pub(crate) fn run_snapshot_projections_for_test(
        &self,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        self.snapshot_projections
            .lock()
            .map(|registry| registry.run())
            .unwrap_or_default()
    }

    /// Test-only direct execution path into the action registry.
    ///
    /// Bypasses [`crate::kernel::ActionRegistry::start`] (which needs a
    /// registered *module* to validate the JSON shape) so a unit test can
    /// exercise a host-registered *executor* on its own — the v1 seam only
    /// exposes executor registration, not module registration. A fixed
    /// placeholder `correlation_id` stands in for the registry-minted id that
    /// the real `dispatch_action` path threads in.
    #[cfg(test)]
    pub(crate) fn test_execute_action(
        &self,
        namespace: &str,
        action_json: &str,
    ) -> Result<(), String> {
        self.action_registry
            .execute(namespace, action_json, "test-correlation-id", &|cmd| {
                self.send_cmd(cmd)
            })
    }

    pub(crate) fn set_pending_mls_autopublish(&self, enabled: bool) {
        self.pending_mls_autopublish
            .store(enabled, Ordering::Release);
    }

    /// Reads the one-shot MLS-autopublish intent and clears it in the same
    /// atomic step (`swap`), so a second caller cannot re-observe the flag.
    /// Atomics cannot poison, so — unlike the previous `Mutex<bool>` — there
    /// is no lock-failure fallback path that could silently drop the intent.
    pub fn take_pending_mls_autopublish(&self) -> bool {
        self.pending_mls_autopublish.swap(false, Ordering::AcqRel)
    }

    /// Clone of the actor command sender. Used by `nmp-signer-broker` to push
    /// `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor without
    /// importing private internals. Stage 4 of the NIP-46 wiring (D0 stays
    /// clean — the broker depends on `nmp-core` + `nmp-signers`; `nmp-core`
    /// has no idea the broker exists).
    ///
    /// G-S4 caveat: sends through this raw clone bypass the `queue_depth`
    /// straddle counter (`send_cmd` is the only incrementing path). The
    /// `actor_queue_depth` snapshot metric is therefore a lower bound when a
    /// broker is wired — acceptable for a backpressure gate that watches for
    /// buildup, not exact occupancy.
    pub fn actor_sender(&self) -> Sender<ActorCommand> {
        self.tx.clone()
    }

    /// Import a local secret through the actor-owned identity reducer.
    pub fn sign_in_nsec(&self, secret: Zeroizing<String>) {
        self.send_cmd(ActorCommand::SignInNsec { secret });
    }

    /// Restore an app-scoped local secret from the keyring capability or use
    /// an injected test secret, then sign it in through the identity reducer.
    pub fn restore_local_nsec_from_keyring(
        &self,
        account_id: &str,
        test_nsec: Option<String>,
    ) -> Option<String> {
        let secret = match test_nsec {
            Some(secret) => Some(secret),
            None => self.recall_local_nsec(account_id),
        }?;
        self.sign_in_nsec(Zeroizing::new(secret.clone()));
        Some(secret)
    }

    /// Persist a newly-imported local secret through the keyring capability,
    /// then sign it in through the identity reducer.
    pub fn sign_in_local_nsec_with_keyring(&self, account_id: &str, secret: String) -> String {
        let req = crate::substrate::KeyringIdentityWiring::persist_secret(
            "nmp.identity.persist",
            account_id,
            &secret,
        );
        let _ = self.dispatch_capability(&req);
        self.sign_in_nsec(Zeroizing::new(secret.clone()));
        secret
    }

    /// Remove an identity through the actor-owned identity reducer.
    pub fn remove_account(&self, identity_id: String) {
        self.send_cmd(ActorCommand::RemoveAccount { identity_id });
    }

    /// Forget the app-scoped local secret and remove the identity through the
    /// actor-owned reducer.
    pub fn remove_account_forgetting_keyring(&self, account_id: &str, identity_id: String) {
        let req = crate::substrate::KeyringIdentityWiring::forget_secret(
            "nmp.identity.forget",
            account_id,
        );
        let _ = self.dispatch_capability(&req);
        self.remove_account(identity_id);
    }

    fn recall_local_nsec(&self, account_id: &str) -> Option<String> {
        let req = crate::substrate::KeyringIdentityWiring::recall_secret(
            "nmp.identity.recall",
            account_id,
        );
        let envelope = self.dispatch_capability(&req);
        let result = crate::substrate::KeyringIdentityWiring::decode_result(&envelope);
        match result.status {
            crate::substrate::KeyringStatus::Ok => result.secret,
            crate::substrate::KeyringStatus::NotFound | crate::substrate::KeyringStatus::Error => {
                None
            }
        }
    }

    /// T146 — register a typed Rust observer. Returns an opaque id the
    /// caller retains to unregister later via
    /// [`Self::unregister_event_observer`]. Used by per-app crates such as
    /// `nmp-app-chirp` which depend on `nmp-core` + a protocol crate
    /// (`nmp-nip01`) and need typed `&KernelEvent` access on the kernel's
    /// ingest fan-out. D0 — `nmp-core` never names the protocol crate; this
    /// trait is the seam.
    pub fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        register_rust_observer(&self.event_observers, observer)
    }

    /// T146 — unregister a previously-registered observer. Idempotent;
    /// unknown ids are silent no-ops (D6).
    pub fn unregister_event_observer(&self, id: KernelEventObserverId) {
        unregister_observer(&self.event_observers, id);
    }

    /// T146 — clone of the kernel event observer slot. The `ffi::event_observer`
    /// FFI surface uses this to plug C-ABI registrations into the same slot
    /// that backs the typed Rust API above. Crate-private because external
    /// Rust callers should go through
    /// [`Self::register_event_observer`] / [`Self::unregister_event_observer`].
    pub(crate) fn event_observers_slot(&self) -> KernelEventObserverSlot {
        Arc::clone(&self.event_observers)
    }

    /// Register a typed Rust raw signed-event observer with a kind filter
    /// (empty filter → all kinds). Returns an opaque id the caller retains
    /// to unregister via [`Self::unregister_raw_event_observer`]. Used by
    /// per-app / protocol crates that need the verbatim signed event
    /// (`sig` included) — e.g. an inbound-ingest seam that must hand the
    /// whole signed event to its own state machine. D0 — `nmp-core` never
    /// names the protocol; this trait is the generic seam.
    pub fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId {
        register_rust_raw_observer(&self.raw_event_observers, kinds, observer)
    }

    /// Unregister a previously-registered raw observer. Idempotent;
    /// unknown ids are silent no-ops (D6).
    pub fn unregister_raw_event_observer(&self, id: RawEventObserverId) {
        unregister_raw_observer(&self.raw_event_observers, id);
    }

    /// Clone of the raw signed-event tap slot. The `ffi::raw_event_tap`
    /// FFI surface uses this to plug C-ABI registrations into the same
    /// slot that backs the typed Rust API above. Crate-private — external
    /// Rust callers go through [`Self::register_raw_event_observer`] /
    /// [`Self::unregister_raw_event_observer`].
    pub(crate) fn raw_event_observers_slot(&self) -> RawEventObserverSlot {
        Arc::clone(&self.raw_event_observers)
    }

    /// Atomically swap the per-app's NIP-17 DM-inbox raw-observer id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Used by per-app crates (e.g. `nmp-app-chirp`) to make their
    /// `register_dm_inbox` entry point idempotent across re-invokes (sign-in,
    /// account switch). Recipe:
    ///
    /// ```ignore
    /// let new_id = app.register_raw_event_observer(filter, projection);
    /// if new_id.0 == 0 { return; }
    /// if let Some(prev) = app.swap_nip17_dm_inbox_observer(Some(new_id)) {
    ///     app.unregister_raw_event_observer(prev);
    /// }
    /// ```
    ///
    /// The swap-then-unregister order is deliberate: storing the new id first
    /// means a host that aborts between register and unregister still has the
    /// new observer live (no inbox-gap window), and the take-and-set under a
    /// single lock acquisition makes the previous id impossible to lose to a
    /// concurrent re-invoke. A poisoned mutex degrades to `None` (D6).
    pub fn swap_nip17_dm_inbox_observer(
        &self,
        new: Option<RawEventObserverId>,
    ) -> Option<RawEventObserverId> {
        let mut guard = self.nip17_dm_inbox_observer_id.lock().ok()?;
        let prev = guard.take();
        *guard = new;
        prev
    }

    /// Atomically swap the per-app's singleton kernel-event observer-id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Mirrors [`Self::swap_nip17_dm_inbox_observer`] for the typed
    /// [`KernelEventObserver`] fan-out (in contrast to the raw signed-event
    /// tap). Same idempotent-re-invoke contract: a per-app crate that wires
    /// exactly one auxiliary `KernelEventObserver` per app uses this slot
    /// to ensure a second registration unregisters the first one before
    /// installing itself. A poisoned mutex degrades to `None` (D6).
    ///
    /// The slot is substrate-generic (D0 — the kernel never names the app
    /// noun); the per-app crate decides what protocol surface the singleton
    /// observer projects.
    pub fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        let mut guard = self.singleton_event_observer_id.lock().ok()?;
        let prev = guard.take();
        *guard = new;
        prev
    }

    /// Push a `LogicalInterest` into the subscription registry and schedule a
    /// recompile. Idempotent: same `InterestId` replaces the prior entry.
    ///
    /// Used by protocol crates (e.g. `nmp-marmot`) to register persistent
    /// relay subscriptions — kind:1059 `#p <pubkey>` for gift-wrap Welcome
    /// delivery, per-group kind:445 feeds, etc. The kernel emits REQ frames
    /// on the next compile pass; matching inbound events then flow through the
    /// raw-event tap automatically, with no Swift polling needed.
    pub fn push_interest(&self, interest: crate::planner::LogicalInterest) {
        self.send_cmd(crate::actor::ActorCommand::PushInterest(interest));
    }

    /// Route a typed capability request through the registered native
    /// callback. Protocol/app composition crates use this when Rust owns the
    /// policy and native only executes the platform capability.
    pub fn dispatch_capability(
        &self,
        request: &crate::substrate::CapabilityRequest,
    ) -> crate::substrate::CapabilityEnvelope {
        let json = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
        let payload =
            crate::capability_socket::dispatch_capability(&self.capability_callback, &json);
        serde_json::from_str(&payload).unwrap_or_else(|_| crate::substrate::CapabilityEnvelope {
            namespace: request.namespace.clone(),
            correlation_id: request.correlation_id.clone(),
            result_json: r#"{"status":"error","os_status":-50}"#.to_string(),
        })
    }

    /// Return the active local (nsec-backed) secret key in `nsec1…` bech32
    /// form, or `None` when no local account is active (remote signer or no
    /// account). The actor writes this slot synchronously before emitting
    /// each identity-change snapshot, so callers inside `apply()` callbacks
    /// always see the up-to-date value. Used by per-app crates (e.g.
    /// per-app crate registration) so the key stays Rust-owned
    /// (D0 — Swift never sees it for the `createAccount` path).
    pub fn marmot_local_nsec(&self) -> Option<Zeroizing<String>> {
        self.marmot_local_nsec.lock().ok()?.clone()
    }

    /// Clone of the active-local-`nostr::Keys` slot — the NIP-44 key seam
    /// for protocol consumers without the raw-key exception (e.g. NIP-17 DM
    /// inbox decryption).
    ///
    /// Returns a clone of the shared `Arc` so the caller (e.g. a
    /// `DmInboxProjection`) holds its own handle and reads the current keys
    /// at decrypt time. The actor is the sole writer; it updates the inner
    /// `Option<Keys>` on every identity mutation, so a long-lived consumer
    /// always observes the up-to-date account without re-registering.
    ///
    /// This is DELIBERATELY separate from [`Self::marmot_local_nsec`]: that
    /// accessor backs the ADR-0025 Marmot exception; NIP-17 uses this slot.
    pub fn nip17_local_keys(&self) -> Arc<Mutex<Option<nostr::Keys>>> {
        Arc::clone(&self.nip17_local_keys)
    }

    /// Clone of the live relay-edit row slot.
    ///
    /// Per-app Rust controllers use this to derive protocol-specific relay
    /// projections without asking platform shells to parse `RelayEditRow.role`.
    /// The actor is the sole writer; callers should take quick snapshots only.
    ///
    /// The slot type is [`crate::kernel::RelayEditRowsSlot`] — a
    /// newtype `Arc<Mutex<RelayEditRowList>>`. Readers iterate via
    /// `guard.as_slice()` so they never touch the inner `Vec` directly. D14
    /// (`crates/nmp-testing/bin/doctrine-lint/rules/d14.rs`) forbids new bare
    /// `Arc<Mutex<Vec<…>>>` fields on `NmpApp`; the typed alias makes the
    /// slot's purpose visible at every call site.
    pub fn relay_edit_rows_handle(&self) -> crate::kernel::RelayEditRowsSlot {
        Arc::clone(&self.relay_edit_rows)
    }

    /// Return the user's current write-relay URLs, read from the shared kernel relay-edit
    /// projection. Empty when the user has not configured any write relays.
    /// Used by per-app crates so relay resolution stays Rust-owned (D0).
    ///
    /// The underlying slot is a typed `RelayEditRowList`; the
    /// reader iterates via `as_slice()` so it never touches the inner `Vec`
    /// directly.
    pub fn write_relay_urls(&self) -> Vec<String> {
        let Ok(guard) = self.relay_edit_rows.lock() else {
            return Vec::new();
        };
        guard
            .as_slice()
            .iter()
            .filter(|r| crate::actor::has_role(&r.role, "write"))
            .map(|r| r.url.clone())
            .collect()
    }

    /// Workspace-internal kernel publish API — verbatim publish of an
    /// already-signed `nostr::Event` to an EXPLICIT relay set. Empty or
    /// malformed relay sets fail closed in the actor publish handler; callers
    /// that want `Auto` routing must use the typed `nmp.publish` action path
    /// with `PublishTarget::Auto`.
    ///
    /// One door per capability — this is the Rust-typed replacement for
    /// the deleted `nmp_app_publish_signed_event*` `extern "C"` symbols. App
    /// composition crates that retain an `NmpApp` (e.g. `nmp-marmot`'s
    /// `MarmotProjection`) reach the kernel through this method instead of
    /// re-declaring those symbols in their own `extern "C"` blocks. The
    /// Schnorr signature + event-id hash are verified on the actor side
    /// (same `commands::publish::publish_signed_event` path the deleted FFI
    /// symbols used to land on); forged or garbled events are dropped with a
    /// kernel toast.
    ///
    /// Routing is fail-closed: this entrypoint always builds a
    /// `PublishTarget::Explicit { relays }`, bypassing the outbox resolver.
    /// Marmot uses this for relay-pinned kind:445 commits / messages and as
    /// the documented kind:1059 inbox-routing approximation. Callers that
    /// want NIP-65 outbox (`PublishTarget::Auto`) must use the typed
    /// `nmp.publish` action path through `dispatch_action` so `Auto` and
    /// `Explicit` never share the same empty-vector encoding.
    ///
    /// kind:1059 envelopes additionally hit the kernel-side D10 defensive
    /// guard in `commands::publish::publish_signed_event`: it refuses any
    /// kind:1059 envelope whose `relays` slice is empty, sets a D6 toast on
    /// the kernel, and drops the envelope — the same behaviour the
    /// call-site guard in `commands::dm::send_gift_wrapped_dm` gives the
    /// NIP-17 send path. The Marmot bridge's own runtime guard in
    /// `nmp-marmot::projection::publish::publish_to` is the matching guard
    /// for the C-ABI symbol path; together they make a kind:1059 Auto-route
    /// structurally impossible regardless of which entry point a caller
    /// reaches the kernel through.
    ///
    /// Theme A discriminator (see `substrate/action.rs`): this is the
    /// system-authored / lifecycle exception to "every event-producing
    /// publish goes through `dispatch_action`". Marmot publishes MLS-signed
    /// events whose outer signature was minted by an ephemeral key (gift
    /// wraps) or by an MLS group credential — neither of which the kernel's
    /// signer can re-mint. The generic action seam (`nmp.publish`) signs +
    /// publishes; this entrypoint publishes verbatim without re-signing.
    ///
    /// Fire-and-forget (D6): a poisoned actor channel is a silent drop, the
    /// same as the deleted FFI symbols. `correlation_id` is always `None`
    /// here — this path is not the `dispatch_action` action-result channel.
    pub fn publish_signed_explicit(&self, event: nostr::Event, relays: &[nostr::RelayUrl]) {
        // RawEvent (flat NIP-01) is what `ActorCommand::PublishSignedEvent`
        // carries; `commands::publish::publish_signed_event` runs the
        // `VerifiedEvent::try_from_raw` gate (signature + id hash) before
        // anything else, so a Marmot caller that constructed `event` from a
        // dispatch op's signed-JSON output is still subject to the same
        // crypto bar as a wire-arrived event. The `tags` clone mirrors
        // every other RawEvent construction site in the crate
        // (`commands::publish` action_registry.rs:420).
        let raw = crate::store::RawEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: event.kind.as_u16() as u32,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };
        let relays: Vec<crate::publish::RelayUrl> = relays.iter().map(|r| r.to_string()).collect();
        self.send_cmd(ActorCommand::PublishSignedEvent {
            raw,
            target: crate::publish::PublishTarget::Explicit { relays },
            correlation_id: None,
        });
    }

    /// Choose the relay for a client-initiated NIP-46 `nostrconnect://`
    /// handshake from the shared kernel relay-edit projection. Empty or
    /// poisoned state falls back through the same Rust-owned policy as an app
    /// with no configured write relays.
    pub fn nostrconnect_relay_url(&self) -> String {
        let Ok(guard) = self.relay_edit_rows.lock() else {
            return crate::NOSTRCONNECT_DEFAULT_RELAY_URL.to_string();
        };
        // Typed slot — iterate via `as_slice()` so the inner `Vec`
        // never leaks through this consumer.
        crate::actor::nostrconnect_relay_url(
            guard
                .as_slice()
                .iter()
                .map(|row| (row.url.as_str(), row.role.as_str())),
        )
    }
}

// SAFETY: `app` is a raw pointer from `nmp_app_new()`. The function is `extern "C"` (callable
// from Swift/C) so it cannot be marked `unsafe` at the Rust level; the caller guarantees the
// pointer contract. The `allow` suppresses the clippy::not_unsafe_ptr_arg_deref lint which
// does not distinguish between `extern "C"` FFI boundaries and ordinary Rust functions.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        // SAFETY: caller guarantees app is a valid pointer allocated by nmp_app_new().
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.update_callback.lock() else {
        return;
    };
    *slot = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
}

/// Set the persistent storage directory for the LMDB `EventStore` backend.
///
/// Threads the host-supplied path through to the kernel so the
/// `lmdb-backend` feature can be used in production (iOS / Android). When
/// the crate is built without `--features lmdb-backend` the path is stored
/// but inert — the in-memory store is always used.
///
/// Call ordering: this MUST be called before [`nmp_app_start`]. The kernel
/// resolves its `EventStore` once, on the actor thread, when the first
/// `Start` would otherwise need it; a path set after the kernel is built
/// has no effect until the next process launch. A `NULL` or empty `path`
/// clears any previously-set path (the kernel then falls back to the
/// `NMP_LMDB_PATH` env var, or the in-memory store).
///
/// Mirrors the `app_ref` + `Mutex::lock` pattern of the other
/// `nmp_app_set_*` setters — no panic can cross the C ABI boundary because
/// the body performs no foreign callback and no panicking operation.
///
/// # Safety
/// `app` must be a valid non-null pointer from [`nmp_app_new`], or null
/// (a null `app` is a silent no-op). `path` must be a valid UTF-8
/// null-terminated C string, or null. Invalid UTF-8 is treated as "unset".
#[no_mangle]
pub extern "C" fn nmp_app_set_storage_path(app: *mut NmpApp, path: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    // `c_optional_string_argument` collapses NULL / empty / whitespace to
    // `None` and returns `Some(trimmed)` otherwise — exactly the
    // "empty clears, non-empty sets" semantics documented above. It also
    // rejects invalid UTF-8 (→ `None`), so no panic is possible here.
    let resolved = c_optional_string_argument(path);
    let Ok(mut slot) = app.storage_path.lock() else {
        return;
    };
    *slot = resolved;
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.send_cmd(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.send_cmd(ActorCommand::Configure {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Stop);
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Reset);
}

pub(crate) fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null app is a valid NmpApp pointer.
        Some(unsafe { &*app })
    }
}

// ADR-0027 deleted `app_ref_mut`. Its only callers were the C-ABI
// `nmp_app_register_action_executor` / `nmp_app_register_action_module`
// registration symbols, which were themselves deleted as part of collapsing
// the dual-seam closure path. The typed registration seam
// (`NmpApp::register_action::<M>`) is Rust-only and takes `&mut self`
// directly; no C-ABI counterpart exists, so no `*mut NmpApp` → `&mut NmpApp`
// helper is needed.

pub(crate) fn c_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    // Validation: to_str() will reject invalid UTF-8.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Optional-string FFI argument. Unlike `c_string_argument` (which collapses
/// NULL / empty / whitespace to `None` for a REQUIRED arg and the caller
/// drops the call), this returns `Some(value)` for non-empty content and
/// `None` for absent — so a NULL `reply_to_id` means "top-level note" rather
/// than "drop the publish". Build-doc §1.1 contract.
pub(crate) fn c_optional_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    let value = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn clamp_visible(visible_limit: c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

fn clamp_emit_hz(emit_hz: c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
