use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::sys::jlong;

use nmp_app_chirp::{nmp_app_chirp_unregister, ChirpHandle};
use nmp_ffi::{
    nmp_app_free, nmp_app_set_capability_callback, nmp_app_set_update_callback, NmpApp,
};

use crate::capability::CapabilityHandlerSlot;

struct CallbackState {
    tx: Mutex<Option<Sender<Vec<u8>>>>,
}

impl CallbackState {
    fn new(tx: Sender<Vec<u8>>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }

    fn send(&self, bytes: Vec<u8>) {
        let Ok(guard) = self.tx.lock() else {
            return;
        };
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(bytes);
        }
    }

    fn close(&self) {
        if let Ok(mut guard) = self.tx.lock() {
            guard.take();
        }
    }
}

struct SessionState {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    updates_closed: bool,
    freed: bool,
}

/// Outbound `external_signer` capability requests (ADR-0048 Stage 2),
/// drained by the Kotlin reader via `nativeNextSignerRequest` — the same
/// channel + blocking-timed-drain shape as the update-frame channel (it
/// sidesteps JNI thread-attach/global-ref complexity; see the module doc
/// in `lib.rs`).
pub(crate) struct SignerRequestChannel {
    tx: Mutex<Option<Sender<String>>>,
    rx: Mutex<Receiver<String>>,
}

impl SignerRequestChannel {
    fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        Self {
            tx: Mutex::new(Some(tx)),
            rx: Mutex::new(rx),
        }
    }

    /// Push one `ExternalSignerRequest` JSON payload for the Kotlin drain.
    pub(crate) fn push(&self, payload_json: String) -> bool {
        let Ok(guard) = self.tx.lock() else {
            return false;
        };
        match guard.as_ref() {
            Some(tx) => tx.send(payload_json).is_ok(),
            None => false,
        }
    }

    fn close(&self) {
        if let Ok(mut guard) = self.tx.lock() {
            guard.take();
        }
    }
}

/// Owns the Android JNI kernel lifetime.
///
/// Kotlin receives an integer registry id, not this allocation's address. Every
/// JNI entry point clones an [`Arc<Session>`] from the registry before touching
/// native state, so `nativeFree` can remove the handle id without reclaiming
/// memory still held by a blocked `nativeNextUpdate`.
pub(crate) struct Session {
    state: Mutex<SessionState>,
    rx: Mutex<Receiver<Vec<u8>>>,
    callback_state: Arc<CallbackState>,
    callback_context: *const CallbackState,
    /// ADR-0048 Stage 2 — outbound NIP-55 capability requests for the Kotlin
    /// drain. The capability trampoline (`external_signer::on_capability_request`)
    /// pushes; `nativeNextSignerRequest` drains.
    pub(crate) signer_requests: SignerRequestChannel,
    /// Synchronous capability handler for non-`external_signer` namespaces
    /// (e.g. Android Keystore keyring). Registered by `nativeSetCapabilityHandler`;
    /// cleared in `close_updates_locked` after the capability socket is unregistered.
    pub(crate) capability_handler: CapabilityHandlerSlot,
    /// Opaque `*mut MarmotHandle` (or null when no MLS identity is registered).
    /// Stored type-erased so the core session module stays feature-agnostic.
    #[cfg_attr(not(feature = "marmot"), allow(dead_code))]
    pub(crate) marmot: AtomicPtr<c_void>,
}

// SAFETY: All mutable lifecycle state is behind `Mutex`/atomics. Raw pointers
// are only consumed while holding `state`, and `free_native` removes them from
// the state before calling the final C-ABI destructors.
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
            signer_requests: SignerRequestChannel::new(),
            capability_handler: Mutex::new(None),
            marmot: AtomicPtr::new(std::ptr::null_mut()),
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

    pub(crate) fn close_updates(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        self.close_updates_locked(&mut state);
    }

    pub(crate) fn free_native(&self) {
        let (app, chirp) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.freed {
                return;
            }
            self.close_updates_locked(&mut state);
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
            nmp_app_free(app);
        }
    }

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

    /// Blocking timed drain of the outbound NIP-55 signer-request channel
    /// (ADR-0048 Stage 2). Same contract as [`Self::recv_next_update`]:
    /// `Idle` is a normal timeout tick, `Closed` means the channel sender
    /// was dropped (session teardown) and the Kotlin reader must stop.
    pub(crate) fn recv_next_signer_request(&self, timeout: Duration) -> NextSignerRequest {
        let Ok(rx) = self.signer_requests.rx.lock() else {
            return NextSignerRequest::Closed;
        };
        match rx.recv_timeout(timeout) {
            Ok(payload) => NextSignerRequest::Request(payload),
            Err(RecvTimeoutError::Timeout) => NextSignerRequest::Idle,
            Err(RecvTimeoutError::Disconnected) => NextSignerRequest::Closed,
        }
    }

    fn close_updates_locked(&self, state: &mut SessionState) {
        if state.updates_closed {
            return;
        }
        if !state.app.is_null() {
            // Quiescence contract (nmp-ffi ADR — UpdateCallbackGate):
            // `nmp_app_set_update_callback(…, None)` does NOT return until any
            // in-flight `on_update` invocation has completed.  After this call
            // returns, `context` (the raw `*const CallbackState` pointer baked
            // into the registration) will never be dereferenced again by the
            // listener thread.  It is therefore safe for `Session::drop` to
            // `drop(Arc::from_raw(self.callback_context))` immediately after
            // `free_native()` returns — the quiescence guarantee prevents the
            // use-after-free race that existed before the gate was introduced.
            nmp_app_set_update_callback(state.app, std::ptr::null_mut(), None);
            // ADR-0048 Stage 2 — unregister the external-signer capability
            // trampoline before the app is freed. The trampoline context is
            // the registry handle id (not a raw pointer), so any in-flight
            // dispatch degrades to an error envelope via the registry lookup
            // rather than a use-after-free.
            nmp_app_set_capability_callback(state.app, std::ptr::null_mut(), None);
        }
        self.callback_state.close();
        self.signer_requests.close();
        // Drop the synchronous capability handler GlobalRef after the
        // capability socket has been unregistered (above). The quiescence
        // guarantee ensures no in-flight trampoline call can race with this.
        if let Ok(mut slot) = self.capability_handler.lock() {
            slot.take();
        }
        state.updates_closed = true;
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
            signer_requests: SignerRequestChannel::new(),
            capability_handler: Mutex::new(None),
            marmot: AtomicPtr::new(std::ptr::null_mut()),
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

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum NextUpdate {
    Frame(Vec<u8>),
    Idle,
    Closed,
}

/// Result of one `recv_next_signer_request` drain tick (ADR-0048 Stage 2).
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum NextSignerRequest {
    Request(String),
    Idle,
    Closed,
}

extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
    if context.is_null() || bytes.is_null() {
        return;
    }
    let state = unsafe { &*(context as *const CallbackState) };
    let owned = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    state.send(owned);
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
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::{insert_session, remove_session, session_arc, NextUpdate, Session};

    #[test]
    fn close_updates_wakes_blocked_next_update() {
        let session = Session::test_session();
        let (entered_tx, entered_rx) = mpsc::channel();
        let reader = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                entered_tx.send(()).expect("signal reader entry");
                session.recv_next_update(Duration::from_secs(60))
            })
        };

        entered_rx.recv().expect("reader entered next update");
        session.close_updates();

        assert_eq!(
            reader.join().expect("reader thread joined"),
            NextUpdate::Closed
        );
    }

    #[test]
    fn free_native_is_idempotent_and_does_not_reclaim_blocked_reader_state() {
        let session = Session::test_session();
        let handle = insert_session(Arc::clone(&session));
        let reader_session = session_arc(handle).expect("registry handle exists");
        let (entered_tx, entered_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            entered_tx.send(()).expect("signal reader entry");
            reader_session.recv_next_update(Duration::from_secs(60))
        });

        entered_rx.recv().expect("reader entered next update");
        let removed = remove_session(handle).expect("first remove returns session");
        removed.free_native();
        removed.free_native();

        assert!(session_arc(handle).is_none());
        assert!(remove_session(handle).is_none());
        assert_eq!(
            reader.join().expect("reader thread joined"),
            NextUpdate::Closed
        );
    }
}
