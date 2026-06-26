//! FFI-side driver helpers shared by the F-04 zap E2E tests (#978).
//!
//! These wrap the production `nmp_app_*` Rust API the iOS shell links —
//! building the full Chirp app, installing an event-driven snapshot signal,
//! reading registered snapshot projections, and blocking (no polling, D8) for
//! a projection to satisfy a predicate. Both the headless round-trip test and
//! the real-wallet last-mile test drive the kernel through exactly these
//! helpers, so the two files share one code path (D4 — single source of
//! truth per behaviour).
//!
//! ## Typed-projection dispatch
//!
//! The generic JSON lane (`nmp_app_read_projection_json`) has been deleted.
//! `read_projection` now dispatches by key to the appropriate typed decoder:
//!
//! - `"wallet"` — Tier-1 host-registered; decoded via
//!   `nmp_nip47::decode_wallet_status`.
//! - `"action_lifecycle"` — Tier-2 kernel built-in; only present in the
//!   emitted frame bytes. The last frame is cached per-app by `on_emit`
//!   (in `FRAME_CACHE`) and decoded via
//!   `nmp_core::typed_projections::decode_action_lifecycle`.
//! - `"last_error_toast"` — Tier-3 FlatBuffers envelope field; decoded from
//!   the cached frame bytes via `nmp_core::decode_snapshot_typed_projections`.
//!   Returns `None` when no frame has been emitted yet (callers use it for
//!   debug logging only, so degrading gracefully is acceptable).

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, Once};
use std::time::Instant;

use nmp_app_chirp::{nmp_app_chirp_register, ChirpHandle, NmpRegisterStatus};
use nmp_ffi::{nmp_app_new, nmp_app_set_update_callback, nmp_app_signin_nsec, NmpApp};

/// Per-app last-frame-bytes cache. Keyed by `app as usize` (stable for the
/// test process lifetime; tests never free an app mid-test). The `Mutex` is
/// only contended between the kernel actor thread (writer) and the test thread
/// (reader), and only for the duration of a `Vec::clone` or `Vec::clone_from`.
static FRAME_CACHE: Mutex<Option<HashMap<usize, Vec<u8>>>> = Mutex::new(None);

/// Context block carried as the `context *mut c_void` in the emit callback.
/// A `Box<EmitContext>` is leaked (process-lifetime) so the raw pointer stays
/// valid for the duration of the test.
struct EmitContext {
    tx: Sender<()>,
    app_key: usize,
}

/// Install the rustls ring provider once (mirrors the relay-worker setup the
/// real-relay smoke tests use). Harmless if the kernel already installed one.
pub fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// The kernel invokes this on every snapshot emit with the frame bytes.
/// Stores the bytes in `FRAME_CACHE` keyed by app pointer, then signals the
/// test thread. `context` is a leaked `Box<EmitContext>` installed by
/// [`install_emit_signal`].
extern "C" fn on_emit(context: *mut c_void, ptr: *const u8, len: usize) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the `EmitContext` we leaked in `install_emit_signal`;
    // it outlives every callback because we never free it during the test.
    let ctx = unsafe { &*(context as *const EmitContext) };

    // Cache the frame bytes for Tier-2 / Tier-3 key dispatch.
    if !ptr.is_null() && len > 0 {
        // SAFETY: `ptr` and `len` come from the kernel's frame buffer which
        // remains valid for the duration of this callback invocation.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        if let Ok(mut guard) = FRAME_CACHE.lock() {
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(ctx.app_key, bytes);
        }
    }

    let _ = ctx.tx.send(());
}

/// Register an emit signal on `app`. Returns the receiver the test blocks on.
/// Leaks the `EmitContext` box intentionally (process-lifetime; freed at exit)
/// so the raw `context` pointer the kernel holds stays valid.
pub fn install_emit_signal(app: *mut NmpApp) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel::<()>();
    let ctx = EmitContext {
        tx,
        app_key: app as usize,
    };
    let boxed = Box::into_raw(Box::new(ctx));
    nmp_app_set_update_callback(app, boxed as *mut c_void, Some(on_emit));
    rx
}

/// Retrieve the most recently cached frame bytes for `app`, or `None` if no
/// frame has been emitted yet.
fn last_frame_bytes(app: *mut NmpApp) -> Option<Vec<u8>> {
    FRAME_CACHE
        .lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .and_then(|map| map.get(&(app as usize)).cloned())
        })
}

/// Read one snapshot projection by key, or `None` if unregistered/absent.
///
/// Dispatch table:
/// - `"wallet"` — Tier-1 typed: `nmp_nip47::decode_wallet_status`
/// - `"action_lifecycle"` — Tier-2 built-in from frame bytes:
///   `nmp_core::typed_projections::decode_action_lifecycle`
/// - `"last_error_toast"` — Tier-3 envelope field from frame bytes
pub fn read_projection(app: *mut NmpApp, key: &str) -> Option<serde_json::Value> {
    // SAFETY: `app` is a valid pointer from `nmp_app_new`; the tests never
    // call `read_projection` after freeing `app`.
    let app_ref: &NmpApp = unsafe { &*app };
    match key {
        "wallet" => {
            let projections = app_ref.run_typed_snapshot_projections();
            let entry = projections
                .iter()
                .find(|p| p.key == "wallet" && !p.payload.is_empty())?;
            let status = nmp_nip47::decode_wallet_status(&entry.payload).ok()?;
            serde_json::to_value(status).ok()
        }
        "action_lifecycle" => {
            // Tier-2 built-in: only present in the emitted frame bytes, not in
            // `run_typed_snapshot_projections()` which covers only Tier-1.
            let frame = last_frame_bytes(app)?;
            let typed_entries =
                nmp_core::decode_snapshot_typed_projections(&frame).ok()?;
            let entry = typed_entries
                .iter()
                .find(|p| p.key == "action_lifecycle" && !p.payload.is_empty())?;
            let model =
                nmp_core::typed_projections::decode_action_lifecycle(&entry.payload).ok()?;
            serde_json::to_value(&model).ok()
        }
        "last_error_toast" => {
            // Tier-3 envelope field: decode the snapshot envelope from the
            // most recently cached frame bytes. Callers use this only for
            // debug logging, so returning None when no frame is cached yet is
            // acceptable (D6: no panic, graceful degradation).
            let frame = last_frame_bytes(app)?;
            let envelope =
                nmp_core::decode_snapshot_envelope(&frame).ok()?;
            envelope.last_error_toast.map(serde_json::Value::String)
        }
        _ => None,
    }
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
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "chirp register must succeed (null viewer)"
    );
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
