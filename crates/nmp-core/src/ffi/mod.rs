//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle wrappers + shared
//! argument helpers; `identity` carries the T66a identity / publish /
//! multi-account / relay-edit wrappers; `timeline` carries the open/close +
//! profile claim/release wrappers; `testing` carries the cfg-gated injectors
//! (split to keep each file under the 300-LOC soft cap).

mod action;
mod capability;
mod event_observer;
mod identity;
mod lifecycle;
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
pub use action::nmp_app_dispatch_action;

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
pub use lifecycle::{
    nmp_app_lifecycle_background, nmp_app_lifecycle_foreground, nmp_app_set_lifecycle_callback,
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

// test-support: expose identity / publish / relay-edit FFI entry-points so
// integration tests (and chirp-repl, which depends on nmp-core with the
// test-support feature) can call them through the rlib without extern "C"
// blocks. The symbols remain `#[no_mangle] extern "C"` in `identity`; this
// `pub use` is only consumed under the test/test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use identity::{
    nmp_app_add_relay, nmp_app_cancel_publish, nmp_app_create_new_account, nmp_app_open_timeline,
    nmp_app_publish_signed_event, nmp_app_publish_signed_event_to, nmp_app_publish_unsigned_event,
    nmp_app_remove_relay, nmp_app_retry_publish, nmp_app_signin_nsec,
};

// android-ffi: expose all FFI entry-points via Rust paths so nmp-android-ffi
// can call them through the rlib. These re-exports are the ONLY thing that
// makes rustc include the symbol bodies in CGU files for the cdylib.
#[cfg(feature = "android-ffi")]
pub use identity::{
    nmp_app_add_relay, nmp_app_cancel_publish, nmp_app_create_new_account,
    nmp_app_open_timeline, nmp_app_publish_signed_event,
    nmp_app_publish_signed_event_to, nmp_app_publish_unsigned_event,
    nmp_app_remove_account, nmp_app_remove_relay, nmp_app_retry_publish, nmp_app_signin_bunker,
    nmp_app_signin_nsec, nmp_app_switch_active,
};
// T118 / G3 — android-ffi must also reach the lifecycle symbols; without this
// re-export rustc doesn't pull the symbol bodies into the cdylib CGU and the
// Android JNI shim can't link.
#[cfg(feature = "android-ffi")]
pub use capability::{
    nmp_app_dispatch_capability, nmp_app_free_string, nmp_app_set_capability_callback,
};
// M6 — action-dispatch entry point + the host-extensible action-executor
// registration seam, reachable via the Rust path so the Android JNI shim
// pulls the symbol bodies into the cdylib CGU. `nmp_app_register_action_executor`
// is `#[no_mangle] extern "C"` in `action` like its `dispatch` sibling; without
// this re-export rustc omits its body from the cdylib CGU and an Android link
// step against it fails (the `cargo check (android-ffi)` CI job never links, so
// it does not catch this). `allow(unused_imports)`: the re-export exists only
// to force the symbol body into the cdylib CGU — no Rust caller names it by
// this path.
#[cfg(feature = "android-ffi")]
#[allow(unused_imports)]
pub use action::{
    nmp_app_dispatch_action, nmp_app_register_action_executor, nmp_app_register_action_module,
};
// Host-extensible snapshot output — registration entry point reachable via
// the Rust path so the Android JNI shim pulls the symbol body into the
// cdylib CGU.
#[cfg(feature = "android-ffi")]
pub use snapshot::nmp_app_register_snapshot_projection;
#[cfg(feature = "android-ffi")]
pub use lifecycle::{
    nmp_app_lifecycle_background, nmp_app_lifecycle_foreground, nmp_app_set_lifecycle_callback,
};
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
    /// Per-app crates (e.g. `nmp-app-chirp`) reach this slot through
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
    /// Shared relay-edit rows handle. Cloned to the actor thread and bound
    /// onto the kernel so external Rust callers (e.g. `nmp-app-chirp` Marmot
    /// dispatch) can read the user's current relay list without crossing FFI.
    relay_edit_rows: Arc<Mutex<Vec<crate::kernel::RelayEditRow>>>,
    /// Active local (nsec-backed) secret key in bech32 form (`nsec1…`). The
    /// actor thread writes this after every identity mutation that changes
    /// the active local key (create, sign-in, switch, remove). Remote-signer
    /// accounts leave this `None`. Per-app crates (e.g. `nmp-app-chirp`
    /// Marmot) read it via [`NmpApp::active_local_nsec`] so they can
    /// register a signer without Swift ever seeing the key.
    ///
    /// Wrapped in [`Zeroizing`] so the bech32 secret is wiped from the heap
    /// when the slot is overwritten or the app drops — a plain `String` would
    /// leave the key recoverable in freed memory.
    active_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>>,
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
    pending_mls_autopublish: Arc<Mutex<bool>>,
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
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut callback) = self.update_callback.lock() {
            *callback = None;
        }
        let _ = self.tx.send(ActorCommand::Shutdown);
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
    // Host-extensible snapshot output slot. Same shared-`Arc` pattern: the
    // `NmpApp` keeps one clone (Rust + C-ABI registration entry points), the
    // actor thread carries another and binds it onto the kernel
    // (`set_snapshot_projection_handle`). Registrations mutate the inner
    // `Mutex<SnapshotRegistry>` visible to both sides.
    let snapshot_projections = crate::kernel::new_snapshot_projection_slot();
    let actor_snapshot_projections = Arc::clone(&snapshot_projections);
    // Shared relay-edit rows handle. Cloned to the actor thread and bound
    // onto the kernel so external Rust callers can read the user's current
    // relay list without crossing FFI.
    let relay_edit_rows: Arc<Mutex<Vec<crate::kernel::RelayEditRow>>> =
        Arc::new(Mutex::new(Vec::new()));
    let actor_relay_edit_rows = Arc::clone(&relay_edit_rows);
    // Active local (nsec) key slot. The actor updates this after every
    // identity mutation; per-app crates read via NmpApp::active_local_nsec.
    let active_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>> = Arc::new(Mutex::new(None));
    let actor_active_local_nsec = Arc::clone(&active_local_nsec);
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
    let pending_mls_autopublish = Arc::new(Mutex::new(false));
    // Clone so we can report actor death through the same listener pipe.
    // The actor `move`s its own `update_tx` into `run_actor_with_observers`;
    // this clone is the supervisor's last live handle once that one is
    // dropped — it MUST outlive the inner closure so the panic frame can
    // still be delivered after the actor's own sender is gone.
    let update_tx_panic = update_tx.clone();
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
                update_tx,
                actor_lifecycle_observer,
                actor_event_observers,
                actor_raw_event_observers,
                actor_snapshot_projections,
                actor_relay_edit_rows,
                actor_active_local_nsec,
                actor_capability_callback,
                actor_storage_path,
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

    Box::into_raw(Box::new(NmpApp {
        tx: command_tx,
        update_callback,
        capability_callback,
        lifecycle_observer,
        event_observers,
        raw_event_observers,
        relay_edit_rows,
        active_local_nsec,
        storage_path,
        pending_mls_autopublish,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
        // M6 — the action registry the kernel ships with: `PublishModule`
        // only. NIP-29 / NIP-59 modules are app nouns (D0) and are
        // registered by the app host against its own registry instance.
        action_registry: crate::kernel::default_registry(),
        // Host-extensible snapshot output: ships empty. A non-social host
        // registers its projections via `nmp_app_register_snapshot_projection`
        // during init; the social shells register nothing and the
        // `projections` snapshot key is `skip_serializing_if`'d off the wire.
        snapshot_projections,
    }))
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
        let _ = self.tx.send(cmd);
    }

    /// Register a host-supplied executor against the app's action registry.
    ///
    /// This is the post-construction registration seam: a host can wire an
    /// action namespace into the registry *without editing `nmp-core`*. The
    /// closure receives the validated action JSON and a `send` callback that
    /// routes an [`ActorCommand`] to the actor; it returns `Ok(())` on
    /// success or `Err(msg)` on a decode/dispatch failure.
    ///
    /// Registration MUST happen during host init — before `nmp_app_start`
    /// and before any [`action::nmp_app_dispatch_action`] call — because it
    /// requires `&mut self`. See [`app_ref_mut`] for the aliasing contract.
    pub fn register_action_executor(
        &mut self,
        namespace: &'static str,
        f: impl Fn(&str, &dyn Fn(ActorCommand)) -> Result<(), String> + Send + Sync + 'static,
    ) {
        self.action_registry.register_executor(namespace, f);
    }

    /// Register a host-supplied *module validator* against the app's action
    /// registry — the complement to [`Self::register_action_executor`].
    ///
    /// `register_action_executor` wires the `execute()` half of a namespace;
    /// this wires the `start()` validation half. A namespace registered
    /// through *both* is fully reachable via
    /// [`action::nmp_app_dispatch_action`]: `start()` validates the action
    /// JSON against `validate`, then `execute()` runs the registered executor.
    /// Registering only one half leaves the namespace partially wired — an
    /// executor-only namespace is rejected by `start()` ("unknown action
    /// namespace"); a validator-only one is rejected by `execute()` ("no
    /// executor registered").
    ///
    /// `validate` receives the raw action JSON and returns an
    /// [`crate::substrate::ActionPlan`] on accept or an
    /// [`crate::substrate::ActionRejection`] on reject.
    ///
    /// Registration MUST happen during host init — before `nmp_app_start`
    /// and before any [`action::nmp_app_dispatch_action`] call — because it
    /// requires `&mut self`. See [`app_ref_mut`] for the aliasing contract.
    pub fn register_action_module(
        &mut self,
        namespace: &'static str,
        validate: impl Fn(
                &str,
            ) -> Result<
                crate::substrate::ActionPlan<serde_json::Value>,
                crate::substrate::ActionRejection,
            > + Send
            + Sync
            + 'static,
    ) {
        self.action_registry
            .register_with_validator(namespace, validate);
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
    /// exposes executor registration, not module registration.
    #[cfg(test)]
    pub(crate) fn test_execute_action(
        &self,
        namespace: &str,
        action_json: &str,
    ) -> Result<(), String> {
        self.action_registry
            .execute(namespace, action_json, &|cmd| self.send_cmd(cmd))
    }

    pub(crate) fn set_pending_mls_autopublish(&self, enabled: bool) {
        if let Ok(mut pending) = self.pending_mls_autopublish.lock() {
            *pending = enabled;
        }
    }

    pub fn take_pending_mls_autopublish(&self) -> bool {
        let Ok(mut pending) = self.pending_mls_autopublish.lock() else {
            return false;
        };
        let value = *pending;
        *pending = false;
        value
    }

    /// Clone of the actor command sender. Used by `nmp-signer-broker` to push
    /// `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor without
    /// importing private internals. Stage 4 of the NIP-46 wiring (D0 stays
    /// clean — the broker depends on `nmp-core` + `nmp-signers`; `nmp-core`
    /// has no idea the broker exists).
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
    /// `nmp-app-chirp` Marmot registration) so the key stays Rust-owned
    /// (D0 — Swift never sees it for the `createAccount` path).
    pub fn active_local_nsec(&self) -> Option<Zeroizing<String>> {
        self.active_local_nsec.lock().ok()?.clone()
    }

    /// Return the user's current write-relay URLs, read from the shared kernel relay-edit
    /// projection. Empty when the user has not configured any write relays.
    /// Used by per-app crates (e.g. `nmp-app-chirp` Marmot dispatch) so
    /// relay resolution stays Rust-owned (D0).
    pub fn write_relay_urls(&self) -> Vec<String> {
        let Ok(guard) = self.relay_edit_rows.lock() else {
            return Vec::new();
        };
        guard
            .iter()
            .filter(|r| crate::actor::has_role(&r.role, "write"))
            .map(|r| r.url.clone())
            .collect()
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

/// Mutable counterpart to [`app_ref`]. Yields a `&mut NmpApp` for FFI entry
/// points that mutate app-owned state (e.g. action-registry registration).
///
/// SAFETY CONTRACT for callers: the resulting `&mut NmpApp` aliases the same
/// allocation any concurrent `app_ref` would hand out. A C-ABI symbol using
/// this MUST be a registration-time-only call — invoked during host init,
/// before `nmp_app_start` and before any `nmp_app_dispatch_action`, so no
/// shared `&NmpApp` is live on another thread.
pub(crate) fn app_ref_mut<'a>(app: *mut NmpApp) -> Option<&'a mut NmpApp> {
    if app.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null app is a valid NmpApp pointer
        // and (per the doc contract above) no aliasing `&NmpApp` is live.
        Some(unsafe { &mut *app })
    }
}

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
