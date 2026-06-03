use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::sys::jlong;

use nmp_app_chirp::{nmp_app_chirp_unregister, ChirpHandle};
use nmp_ffi::{nmp_app_free, nmp_app_set_update_callback, NmpApp};

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

    fn close_updates_locked(&self, state: &mut SessionState) {
        if state.updates_closed {
            return;
        }
        if !state.app.is_null() {
            nmp_app_set_update_callback(state.app, std::ptr::null_mut(), None);
        }
        self.callback_state.close();
        state.updates_closed = true;
    }

    #[cfg(test)]
    fn test_session() -> Arc<Self> {
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
