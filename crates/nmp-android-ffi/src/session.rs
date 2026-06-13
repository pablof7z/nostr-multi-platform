use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(test)]
use std::sync::mpsc::RecvTimeoutError;
#[cfg(test)]
use std::time::Duration;

use jni::sys::jlong;

use nmp_app_chirp::{nmp_app_chirp_unregister, ChirpHandle};
use nmp_ffi::{nmp_app_free, nmp_app_set_capability_callback, nmp_app_set_update_callback, NmpApp};

use crate::capability::CapabilityHandlerSlot;
use crate::signer_request_listener::SignerRequestListenerSlot;
use crate::update_listener::UpdateListenerSlot;
pub(crate) use crate::update_listener::UpdatePushListener;

struct CallbackState {
    /// Legacy mpsc sink — retained only for the in-crate unit tests
    /// (`recv_next_update`). Production update delivery is the JNI push path
    /// via [`CallbackState::push_listener`].
    tx: Mutex<Option<Sender<Vec<u8>>>>,
    /// JNI push listener — invoked on every update frame (issue #614, D8: no
    /// polling). Cleared in [`Session::close_updates_locked`] after the
    /// quiescence gate guarantees no further `on_update` invocations.
    push_listener: UpdateListenerSlot,
}

impl CallbackState {
    fn new(tx: Sender<Vec<u8>>) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
            push_listener: Mutex::new(None),
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

    fn set_push_listener(&self, listener: UpdatePushListener) {
        if let Ok(mut slot) = self.push_listener.lock() {
            *slot = Some(Arc::new(listener));
        }
    }

    fn clear_push_listener(&self) {
        if let Ok(mut slot) = self.push_listener.lock() {
            slot.take();
        }
    }
}

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
    /// Legacy mpsc receiver — drained only by the test-only `recv_next_update`
    /// since issue #614 removed the production polling path. Kept so the unit
    /// tests can exercise the close/quiescence lifecycle without live JNI.
    #[cfg_attr(not(test), allow(dead_code))]
    rx: Mutex<Receiver<Vec<u8>>>,
    callback_state: Arc<CallbackState>,
    callback_context: *const CallbackState,
    /// ADR-0048 Stage 2 — JNI push listener for outbound NIP-55 capability
    /// requests (issue #1284, D8: no polling). The capability trampoline
    /// (`external_signer::on_capability_request`) pushes each request JSON
    /// straight to the registered Kotlin listener; cleared on teardown by
    /// [`Self::close_updates_locked`] after the capability socket is
    /// unregistered.
    pub(crate) signer_request_listener: SignerRequestListenerSlot,
    /// Test-only capture sink for pushed NIP-55 signer requests. Production
    /// pushes go through the JNI `signer_request_listener` (which needs a live
    /// JVM); the unit tests have no JVM, so `push_signer_request` mirrors each
    /// pushed request into this `Vec` when one is present and reports success,
    /// letting the trampoline tests assert the request payload + ack envelope
    /// without a JNI listener. Never compiled into the shipped `.so`.
    #[cfg(test)]
    pub(crate) signer_request_capture: Mutex<Option<Vec<String>>>,
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
            signer_request_listener: Mutex::new(None),
            #[cfg(test)]
            signer_request_capture: Mutex::new(None),
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

    /// Register the JNI push listener for kernel update frames (issue #614).
    /// Replaces an existing listener if one is already set. Cleared on
    /// teardown by [`Self::close_updates_locked`].
    pub(crate) fn set_push_listener(&self, listener: UpdatePushListener) {
        self.callback_state.set_push_listener(listener);
    }

    /// Drop the JNI push listener (deregister). Safe to call when none is set.
    pub(crate) fn clear_push_listener(&self) {
        self.callback_state.clear_push_listener();
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

    /// Test-only blocking drain of the legacy mpsc update channel.
    ///
    /// Issue #614 removed the production `nativeNextUpdate` polling path; the
    /// in-crate unit tests still exercise the channel + quiescence lifecycle
    /// through this helper, so it is gated `#[cfg(test)]`.
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
            // Issue #614 — drop the JNI push listener `GlobalRef` now that the
            // quiescence gate above guarantees no in-flight (or future)
            // `on_update` can read the slot. Doing it INSIDE the `app`-non-null
            // branch (right after the gate) is what makes the drop UAF-safe.
            self.callback_state.clear_push_listener();
            // ADR-0048 Stage 2 — unregister the external-signer capability
            // trampoline before the app is freed. The trampoline context is
            // the registry handle id (not a raw pointer), so any in-flight
            // dispatch degrades to an error envelope via the registry lookup
            // rather than a use-after-free.
            nmp_app_set_capability_callback(state.app, std::ptr::null_mut(), None);
            // Issue #1284 — drop the NIP-55 signer-request push listener
            // `GlobalRef` now that the capability trampoline above is
            // unregistered. The trampoline snapshots an `Arc` clone of the
            // listener under the slot lock and drops the lock before its JNI
            // upcall (mirrors `on_update`), so this `take()` only ever races a
            // cheap `Arc::clone`, never an in-flight `push`.
            self.clear_signer_request_listener();
        }
        self.callback_state.close();
        // Drop the synchronous capability handler GlobalRef.
        //
        // UAF safety is NOT the capability-socket unregister above: unlike the
        // update-callback gate, `nmp_app_set_capability_callback(None)` does
        // NOT quiesce an in-flight capability dispatch (the kernel capability
        // socket clones the registration out and drops its slot lock before
        // invoking the trampoline). The load-bearing guard is THIS `lock()`:
        // `capability::call_sync_handler` holds `capability_handler` for the
        // entire `handler.call()`, so the `take()` here serializes against any
        // active dispatch and the GlobalRef is never dropped while in use.
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
            signer_request_listener: Mutex::new(None),
            #[cfg(test)]
            signer_request_capture: Mutex::new(None),
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

/// Result of one `recv_next_update` drain tick. Test-only since issue #614
/// removed the production polling path (the JNI push listener is now the
/// production update-delivery seam).
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
    // JNI push path (issue #614 — D8: no polling). The kernel pushes the frame
    // straight to the Kotlin listener instead of a Kotlin thread draining a
    // 250 ms-timed channel.
    //
    // Lock ordering: we snapshot an `Arc` clone under the lock, drop the lock
    // BEFORE invoking the JNI callback. This prevents a deadlock where Kotlin
    // re-enters a Rust JNI entry-point (or the actor) that itself tries to
    // acquire `push_listener` — which would deadlock if the lock were still
    // held across the upcall. Pattern mirrors nmp-ffi's update-callback
    // quiescence loop (nmp-ffi/src/lib.rs, "option b — Condvar drain").
    let listener_snapshot: Option<Arc<UpdatePushListener>> =
        state.push_listener.lock().ok().and_then(|g| g.clone());
    if let Some(listener) = listener_snapshot {
        listener.push(frame);
    }
    // Legacy mpsc path — only the in-crate unit tests drain this now.
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
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::{insert_session, remove_session, session_arc, NextUpdate, Session};

    #[test]
    fn push_listener_slot_starts_empty() {
        // A freshly constructed session has no JNI push listener registered.
        let session = Session::test_session();
        let guard = session.callback_state.push_listener.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn close_updates_clears_push_listener_slot() {
        // UAF-safety invariant: after teardown the push-listener slot is empty
        // (in production the `GlobalRef` is dropped after the quiescence gate).
        let session = Session::test_session();
        session.close_updates();
        let guard = session.callback_state.push_listener.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn on_update_forwards_frame_to_mpsc_when_no_push_listener() {
        // With no JNI listener registered, the push branch is a no-op and the
        // frame still reaches the legacy mpsc test seam — never dropped.
        let session = Session::test_session();
        let handle = insert_session(Arc::clone(&session));
        session.callback_state.send(b"frame-bytes".to_vec());
        let update = session.recv_next_update(Duration::from_millis(200));
        assert_eq!(update, NextUpdate::Frame(b"frame-bytes".to_vec()));
        remove_session(handle);
    }

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
