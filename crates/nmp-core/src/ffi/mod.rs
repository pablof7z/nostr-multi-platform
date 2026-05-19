//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle wrappers + shared
//! argument helpers; `identity` carries the T66a identity / publish /
//! multi-account / relay-edit wrappers; `timeline` carries the open/close +
//! profile claim/release wrappers; `testing` carries the cfg-gated injectors
//! (split to keep each file under the 300-LOC soft cap).

mod capability;
mod event_observer;
mod identity;
mod lifecycle;
mod raw_event_tap;
mod timeline;
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
// integration tests can call them through the rlib without extern "C" blocks.
// The symbols remain `#[no_mangle] extern "C"` in `identity`; this `pub use`
// is only consumed under the test/test-support gate.
#[cfg(any(test, feature = "test-support"))]
pub use identity::{
    nmp_app_publish_signed_event, nmp_app_publish_signed_event_to,
    nmp_app_publish_unsigned_event, nmp_app_signin_nsec,
};

// android-ffi: expose all FFI entry-points via Rust paths so nmp-android-ffi
// can call them through the rlib. These re-exports are the ONLY thing that
// makes rustc include the symbol bodies in CGU files for the cdylib.
#[cfg(feature = "android-ffi")]
pub use identity::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_follow, nmp_app_open_timeline,
    nmp_app_publish_note, nmp_app_publish_signed_event, nmp_app_publish_signed_event_to,
    nmp_app_publish_unsigned_event,
    nmp_app_react, nmp_app_remove_account,
    nmp_app_remove_relay, nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
    nmp_app_unfollow,
};
// T118 / G3 — android-ffi must also reach the lifecycle symbols; without this
// re-export rustc doesn't pull the symbol bodies into the cdylib CGU and the
// Android JNI shim can't link.
#[cfg(feature = "android-ffi")]
pub use capability::{
    nmp_app_dispatch_capability, nmp_app_free_string, nmp_app_set_capability_callback,
};
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
#[cfg(feature = "android-ffi")]
pub use wallet::{nmp_app_wallet_connect, nmp_app_wallet_disconnect, nmp_app_wallet_pay_invoice};

use crate::actor::{
    new_event_observer_slot, new_lifecycle_observer_slot, new_raw_event_observer_slot,
    register_rust_observer, register_rust_raw_observer, run_actor_with_observers,
    unregister_observer, unregister_raw_observer, ActorCommand, KernelEventObserver,
    KernelEventObserverId, KernelEventObserverSlot, KindFilter, LifecycleObserverSlot,
    RawEventObserver, RawEventObserverId, RawEventObserverSlot,
};
use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);

#[derive(Clone, Copy)]
struct UpdateCallbackRegistration {
    context: usize,
    callback: UpdateCallback,
}

pub struct NmpApp {
    tx: Sender<ActorCommand>,
    update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>>,
    capability_callback: Arc<Mutex<Option<capability::CapabilityCallbackRegistration>>>,
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
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
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
    // Clone so we can report actor panics through the same listener pipe.
    let update_tx_panic = update_tx.clone();
    let actor = thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_actor_with_observers(
                command_rx,
                update_tx,
                actor_lifecycle_observer,
                actor_event_observers,
                actor_raw_event_observers,
            );
        }));
        if let Err(e) = result {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "unknown panic in actor thread".to_string()
            };
            let frame = format!(
                "{{\"t\":\"panic\",\"m\":\"actor panicked: {}\"}}",
                msg.replace('"', "\\\"").replace('\\', "\\\\")
            );
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
                (registration.callback)(registration.context as *mut c_void, payload.as_ptr());
            }
        }
    });

    Box::into_raw(Box::new(NmpApp {
        tx: command_tx,
        update_callback,
        capability_callback: Arc::new(Mutex::new(None)),
        lifecycle_observer,
        event_observers,
        raw_event_observers,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
    }))
}

impl NmpApp {
    /// Send a command to the actor thread, logging at error level if the
    /// channel is disconnected (actor thread panicked or exited).
    pub(crate) fn send_cmd(&self, cmd: ActorCommand) {
        if let Err(_) = self.tx.send(cmd) {
            eprintln!("NMP_FFI_ERR: actor command channel disconnected — actor thread may have panicked");
        }
    }

    /// Clone of the actor command sender. Used by `nmp-signer-broker` to push
    /// `AddRemoteSigner` / `BunkerHandshakeProgress` back to the actor without
    /// importing private internals. Stage 4 of the NIP-46 wiring (D0 stays
    /// clean — the broker depends on `nmp-core` + `nmp-signers`; `nmp-core`
    /// has no idea the broker exists).
    pub fn actor_sender(&self) -> Sender<ActorCommand> {
        self.tx.clone()
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
