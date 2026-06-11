//! FFI-side driver helpers shared by the F-04 zap E2E tests (#978).
//!
//! These wrap the production `nmp_app_*` C-ABI surface the iOS shell links —
//! building the full Chirp app, installing an event-driven snapshot signal,
//! reading registered snapshot projections, and blocking (no polling, D8) for
//! a projection to satisfy a predicate. Both the headless round-trip test and
//! the real-wallet last-mile test drive the kernel through exactly these
//! helpers, so the two files share one code path (D4 — single source of
//! truth per behaviour).

use std::ffi::{c_void, CStr, CString};
use std::sync::mpsc::{self, Sender};
use std::sync::Once;
use std::time::Instant;

use nmp_app_chirp::{nmp_app_chirp_register, ChirpHandle};
use nmp_ffi::{
    nmp_app_free_string, nmp_app_new, nmp_app_read_projection_json, nmp_app_set_update_callback,
    nmp_app_signin_nsec, NmpApp,
};

/// Install the rustls ring provider once (mirrors the relay-worker setup the
/// real-relay smoke tests use). Harmless if the kernel already installed one.
pub fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// The kernel invokes this on every snapshot emit with the frame bytes. We
/// ignore the bytes and signal the test thread that a new snapshot is ready;
/// the test then reads the projection it cares about. `context` is a leaked
/// `Box<Sender<()>>` installed by [`install_emit_signal`].
extern "C" fn on_emit(context: *mut c_void, _ptr: *const u8, _len: usize) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the `Sender<()>` we leaked in `install_emit_signal`;
    // it outlives every callback because we never free it during the test.
    let tx = unsafe { &*(context as *const Sender<()>) };
    let _ = tx.send(());
}

/// Register an emit signal on `app`. Returns the receiver the test blocks on.
/// Leaks the `Sender` box intentionally (process-lifetime; freed at exit) so
/// the raw `context` pointer the kernel holds stays valid.
pub fn install_emit_signal(app: *mut NmpApp) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel::<()>();
    let boxed = Box::into_raw(Box::new(tx));
    nmp_app_set_update_callback(app, boxed as *mut c_void, Some(on_emit));
    rx
}

/// Read one snapshot projection by key, or `None` if unregistered/absent.
pub fn read_projection(app: *mut NmpApp, key: &str) -> Option<serde_json::Value> {
    let key_c = CString::new(key).ok()?;
    let ptr = nmp_app_read_projection_json(app, key_c.as_ptr());
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is a heap-owned NUL-terminated C string from the FFI; copy
    // out then free via the matching deallocator.
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::to_owned);
    nmp_app_free_string(ptr);
    json.and_then(|s| serde_json::from_str(&s).ok())
}

/// Block until `predicate(projection_value)` holds for `key`, driven by the
/// emit signal. Returns the matching value, or `None` on deadline.
///
/// D8: each iteration blocks on `rx.recv_timeout` (an OS-event wait on the
/// emit channel), never a `sleep`+check spin. The remaining-budget timeout
/// caps the total wait.
pub fn wait_for_projection<F>(
    app: *mut NmpApp,
    rx: &mpsc::Receiver<()>,
    key: &str,
    deadline: Instant,
    mut predicate: F,
) -> Option<serde_json::Value>
where
    F: FnMut(&serde_json::Value) -> bool,
{
    // Check once up front in case the state is already satisfied.
    if let Some(v) = read_projection(app, key) {
        if predicate(&v) {
            return Some(v);
        }
    }
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        // Block for the next emit (or give up at the deadline).
        if rx.recv_timeout(remaining).is_err() {
            // Timed out / disconnected — one last read before declaring failure.
            if let Some(v) = read_projection(app, key) {
                if predicate(&v) {
                    return Some(v);
                }
            }
            return None;
        }
        if let Some(v) = read_projection(app, key) {
            if predicate(&v) {
                return Some(v);
            }
        }
    }
}

/// Build the full Chirp app (the same composition the iOS shell ships) and
/// sign in a fresh local key so the active account exists.
pub fn build_app_signed_in(nsec: &str) -> (*mut NmpApp, *mut ChirpHandle) {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null(), "chirp register must return a handle");
    let nsec_c = CString::new(nsec).expect("nsec NUL-free");
    nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);
    (app, handle)
}

/// Build a `nostr+walletconnect://` URI pointing the kernel at `relay_url`
/// with the given wallet pubkey + client secret.
///
/// The relay value is percent-encoded for the `?relay=…` query form;
/// `NwcUri::parse` URL-decodes it back to `ws://host:port`.
pub fn nwc_uri(wallet_pubkey_hex: &str, relay_url: &str, client_secret_hex: &str) -> String {
    let relay_enc = relay_url.replace(':', "%3A").replace('/', "%2F");
    format!("nostr+walletconnect://{wallet_pubkey_hex}?relay={relay_enc}&secret={client_secret_hex}")
}
