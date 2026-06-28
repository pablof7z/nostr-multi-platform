use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
#[cfg(test)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
#[cfg(test)]
use std::time::Duration;

use jni::sys::jlong;

use nmp_app_chirp::{ChirpHandle, nmp_app_chirp_unregister};
use nmp_ffi::{NmpApp, nmp_app_free, nmp_app_set_capability_callback, nmp_app_set_update_callback};

use crate::capability::CapabilityHandlerSlot;
use crate::signer_request_listener::SignerRequestListenerSlot;
pub(crate) use self::callback_state::CallbackState;

#[path = "session/callback_state.rs"]
mod callback_state;

struct SessionState {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    updates_closed: bool,
    freed: bool,
}

/// Owns the Android JNI kernel lifetime.
///
/// Kotlin receives an integer registry id, not this allocation's address. Every
/// JNI entry point clones an [`Arc<Session>`] from the registry before touching
/// native state, so `nativeFree` can remove the handle id without reclaiming
/// memory still in use by an in-flight JNI call.
pub(crate) struct Session {
    state: Mutex<SessionState>,
    /// Legacy mpsc receiver — test-only since issue #614.
    #[cfg_attr(not(test), allow(dead_code))]
    rx: Mutex<Receiver<Vec<u8>>>,
    pub(crate) callback_state: Arc<CallbackState>,
    callback_context: *const CallbackState,
    /// ADR-0048 Stage 2 — JNI push listener for outbound NIP-55 capability
    /// requests (issue #1284, D8: no polling).
    pub(crate) signer_request_listener: SignerRequestListenerSlot,
    #[cfg(test)]
    pub(crate) signer_request_capture: Mutex<Option<Vec<String>>>,
    pub(crate) capability_handler: CapabilityHandlerSlot,
    #[cfg_attr(not(feature = "marmot"), allow(dead_code))]
    pub(crate) marmot: AtomicPtr<c_void>,
    /// Lifecycle guard for lock-free callback-mutation operations (FIX 1+2).
    ///
    /// Read lock: held by lock-free quiescence/reregister/close_updates Phase 2
    /// while using the raw `app` pointer without holding `Session.state`.
    /// Write lock: held by `free_native` around `nmp_app_free(app)`. No other
    /// lock is held concurrently with the write lock.
    ///
    /// Invariant (UAF prevention): `nmp_app_free` runs only while the write
    /// lock is held. Any thread that extracted a non-null `app` pointer and
    /// acquired the read lock is guaranteed `app` remains valid until the read
    /// lock is released — the exclusive write lock blocks `nmp_app_free` until
    /// all such readers complete.
    pub(crate) callback_mutation_guard: RwLock<()>,
}

// SAFETY: All mutable lifecycle state is behind Mutex/atomics. Raw pointers
// are only dereferenced while holding `callback_mutation_guard` (read lock for
// lock-free users; write lock for `free_native` before `nmp_app_free`).
unsafe impl Send for Session {}
unsafe impl Sync for Session {}

impl Session {
    pub(crate) fn new(app: *mut NmpApp, chirp: *mut ChirpHandle) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let callback_state = Arc::new(CallbackState::new(tx));
        let callback_context = Arc::into_raw(Arc::clone(&callback_state));
        nmp_app_set_update_callback(app, callback_context as *mut c_void, Some(on_update));
        Self {
            state: Mutex::new(SessionState {
                app,
                chirp,
                updates_closed: false,
                freed: false,
            }),
            rx: Mutex::new(rx),
            callback_state,
            callback_context,
            signer_request_listener: Mutex::new(None),
            #[cfg(test)]
            signer_request_capture: Mutex::new(None),
            capability_handler: Mutex::new(None),
            marmot: AtomicPtr::new(std::ptr::null_mut()),
            callback_mutation_guard: RwLock::new(()),
        }
    }

    pub(crate) fn with_app<R>(&self, f: impl FnOnce(*mut NmpApp) -> R) -> Option<R> {
        let Ok(state) = self.state.lock() else {
            return None;
        };
        if state.updates_closed || state.freed || state.app.is_null() {
            return None;
        }
        Some(f(state.app))
    }

    /// Quiesce update/capability callbacks and clear all delivery sinks.
    /// Idempotent.
    ///
    /// ## Deadlock-prevention (FIX 1)
    ///
    /// Three-phase design so `state` is never held during the blocking
    /// `nmp_app_set_update_callback(None)` call:
    /// 1. Brief `state` lock: check `updates_closed`; extract `app`; release.
    /// 2. Read lock on `callback_mutation_guard` (NO `state`): call the
    ///    blocking quiescence functions; re-check `state` under the read lock
    ///    to confirm the pointer is still valid before use (UAF guard FIX 2).
    /// 3. `state` lock again: clear listeners, close mpsc, set `updates_closed`.
    pub(crate) fn close_updates(&self) {
        // Phase 1
        let app = {
            let Ok(state) = self.state.lock() else { return };
            if state.updates_closed { return; }
            state.app
        };

        // Phase 2: blocking quiescence WITHOUT `state`.
        // Read lock prevents concurrent nmp_app_free (write lock in free_native).
        if !app.is_null() {
            let _guard = self.callback_mutation_guard.read().unwrap_or_else(|e| e.into_inner());
            // Re-check: if free_native freed the app or another thread already
            // ran close_updates between Phase 1 and acquiring the read lock, skip.
            let skip = self.state.lock()
                .map(|s| s.updates_closed || s.freed)
                .unwrap_or(true);
            if !skip {
                // These block until any in-flight on_update / capability
                // callback completes. `state` is NOT held here, so an in-flight
                // on_update that calls back into with_app cannot deadlock.
                nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
                nmp_app_set_capability_callback(app, std::ptr::null_mut(), None);
            }
        }

        // Phase 3: cleanup under `state`.
        let Ok(mut state) = self.state.lock() else { return };
        if state.updates_closed { return; } // idempotent
        if !state.app.is_null() {
            self.callback_state.clear_generic_sink();
            self.clear_signer_request_listener();
        }
        self.callback_state.close();
        if let Ok(mut slot) = self.capability_handler.lock() {
            slot.take();
        }
        state.updates_closed = true;
    }

    /// Free all native resources.  Idempotent.
    ///
    /// `nmp_app_free` is called while holding the write lock on
    /// `callback_mutation_guard` so it cannot race any lock-free caller
    /// (quiesce / reregister / close_updates Phase 2) that holds the read lock
    /// and is actively using the same raw pointer (UAF guard — FIX 2).
    pub(crate) fn free_native(&self) {
        self.close_updates(); // idempotent; quiesces before we null state.app

        let (app, chirp) = {
            let Ok(mut state) = self.state.lock() else { return };
            if state.freed { return; }
            state.freed = true;
            let app = state.app;
            let chirp = state.chirp;
            state.app = std::ptr::null_mut();
            state.chirp = std::ptr::null_mut();
            (app, chirp)
        };

        crate::marmot::unregister(self);
        if !chirp.is_null() {
            nmp_app_chirp_unregister(chirp);
        }
        if !app.is_null() {
            // Write lock: blocks until all in-flight read-lock holders (lock-free
            // quiescence / reregister / close_updates Phase 2) complete, then
            // the pointer is exclusively ours to free.
            let _write_guard =
                self.callback_mutation_guard.write().unwrap_or_else(|e| e.into_inner());
            // SAFETY: state.freed=true, state.app=null. No thread that respects
            // those flags will access this pointer. The write lock serialises us
            // after any concurrent reader that extracted the pointer before freed
            // was set.
            nmp_app_free(app);
        }
    }

    #[cfg(test)]
    pub(crate) fn recv_next_update(&self, timeout: Duration) -> NextUpdate {
        let Ok(rx) = self.rx.lock() else {
            return NextUpdate::Closed;
        };
        match rx.recv_timeout(timeout) {
            Ok(bytes) => NextUpdate::Frame(bytes),
            Err(RecvTimeoutError::Timeout) => NextUpdate::Idle,
            Err(RecvTimeoutError::Disconnected) => NextUpdate::Closed,
        }
    }

    pub(crate) fn set_generic_sink(&self, sink: Arc<dyn Fn(Vec<u8>) + Send + Sync>) {
        self.callback_state.set_generic_sink(sink);
    }

    pub(crate) fn clear_generic_sink(&self) {
        self.callback_state.clear_generic_sink();
    }

    /// Quiesce the kernel update callback WITHOUT holding `Session.state`.
    ///
    /// Acquires the read lock on `callback_mutation_guard` BEFORE extracting
    /// `app` so `free_native` (write lock) cannot free the pointer while we
    /// use it (UAF guard FIX 2).  `state` is NOT held during the blocking
    /// `nmp_app_set_update_callback` call (deadlock prevention FIX 1).
    pub(crate) fn quiesce_update_callback_lockfree(&self) {
        // Read lock FIRST — prevents concurrent nmp_app_free.
        let _guard = self.callback_mutation_guard.read().unwrap_or_else(|e| e.into_inner());
        let app = {
            let Ok(state) = self.state.lock() else { return };
            if state.freed { return; }
            state.app
        };
        if !app.is_null() {
            nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        }
    }

    /// Re-register the `on_update` C callback after a lockfree quiescence.
    /// Must NOT be called while `state.updates_closed` is true.
    pub(crate) fn reregister_update_callback_lockfree(&self) {
        let _guard = self.callback_mutation_guard.read().unwrap_or_else(|e| e.into_inner());
        let app = {
            let Ok(state) = self.state.lock() else { return };
            if state.updates_closed || state.freed { return; }
            state.app
        };
        if !app.is_null() {
            nmp_app_set_update_callback(
                app,
                self.callback_context as *mut std::ffi::c_void,
                Some(on_update),
            );
        }
    }

    /// Inert (null-app) session for the `AppHandle` init-failure path (D6).
    pub(crate) fn inert_session() -> Arc<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        Arc::new(Self {
            state: Mutex::new(SessionState {
                app: std::ptr::null_mut(),
                chirp: std::ptr::null_mut(),
                updates_closed: false,
                freed: false,
            }),
            rx: Mutex::new(rx),
            callback_state: Arc::new(CallbackState::new(tx)),
            callback_context: std::ptr::null(),
            signer_request_listener: Mutex::new(None),
            #[cfg(test)]
            signer_request_capture: Mutex::new(None),
            capability_handler: Mutex::new(None),
            marmot: AtomicPtr::new(std::ptr::null_mut()),
            callback_mutation_guard: RwLock::new(()),
        })
    }

    /// Test-only: fire the generic sink directly, bypassing the kernel.
    #[cfg(test)]
    pub(crate) fn callback_state_send_via_generic(&self, bytes: Vec<u8>) {
        let sink: Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>> =
            self.callback_state.generic_sink.lock().ok().and_then(|g| g.clone());
        if let Some(f) = sink {
            f(bytes);
        }
    }

    /// Test-only: check whether the generic sink slot is occupied.
    #[cfg(test)]
    pub(crate) fn callback_state_generic_has_sink(&self) -> bool {
        self.callback_state
            .generic_sink
            .lock()
            .ok()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn test_session() -> Arc<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        Arc::new(Self {
            state: Mutex::new(SessionState {
                app: std::ptr::null_mut(),
                chirp: std::ptr::null_mut(),
                updates_closed: false,
                freed: false,
            }),
            rx: Mutex::new(rx),
            callback_state: Arc::new(CallbackState::new(tx)),
            callback_context: std::ptr::null(),
            signer_request_listener: Mutex::new(None),
            #[cfg(test)]
            signer_request_capture: Mutex::new(None),
            capability_handler: Mutex::new(None),
            marmot: AtomicPtr::new(std::ptr::null_mut()),
            callback_mutation_guard: RwLock::new(()),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.free_native();
        if !self.callback_context.is_null() {
            unsafe {
                drop(Arc::from_raw(self.callback_context));
            }
        }
    }
}

/// Test-only: result of one `recv_next_update` drain tick.
#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum NextUpdate {
    Frame(Vec<u8>),
    Idle,
    Closed,
}

extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
    if context.is_null() || bytes.is_null() {
        return;
    }
    let state = unsafe { &*(context as *const CallbackState) };
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    // UniFFI generic-sink path (M14-0 / issue #2129 — D8: no polling).
    // Snapshot-under-lock, release BEFORE invoke — deadlock prevention.
    let generic_snapshot: Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>> =
        state.generic_sink.lock().ok().and_then(|g| g.clone());
    if let Some(sink) = generic_snapshot {
        sink(frame.to_vec());
    }
    // Legacy mpsc path — only in-crate unit tests drain this.
    state.send(frame.to_vec());
}

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<jlong, Arc<Session>>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<jlong, Arc<Session>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn insert_session(session: Arc<Session>) -> jlong {
    let Ok(mut registry) = sessions().lock() else {
        session.free_native();
        return 0;
    };
    loop {
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
        if handle <= 0 {
            NEXT_HANDLE.store(2, Ordering::SeqCst);
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = registry.entry(handle) {
            slot.insert(session);
            return handle;
        }
    }
}

pub(crate) fn session_arc(handle: jlong) -> Option<Arc<Session>> {
    if handle == 0 {
        return None;
    }
    let registry = sessions().lock().ok()?;
    registry.get(&handle).cloned()
}

pub(crate) fn remove_session(handle: jlong) -> Option<Arc<Session>> {
    if handle == 0 {
        return None;
    }
    let mut registry = sessions().lock().ok()?;
    registry.remove(&handle)
}

#[cfg(test)]
#[path = "push_listener_lock_ordering_tests.rs"]
mod push_listener_lock_ordering_tests; // PR #1226 lock-ordering regression tests

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
