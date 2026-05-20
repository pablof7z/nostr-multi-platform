//! Marmot (MLS-over-Nostr) per-app FFI surface.
//!
//! Six `extern "C"` symbols Swift links against — they mirror the
//! lifetime / free / D6 conventions of the Chirp timeline symbols
//! (`nmp_app_chirp_register` / `_snapshot` / `_snapshot_free` /
//! `_unregister`):
//!
//! - [`nmp_app_chirp_marmot_register`] — build a [`MarmotService`]
//!   (signer seam: secret key hex/nsec passed directly; DB at
//!   `<app_support>/marmot-mls-state.sqlite`), register the lossy
//!   `KernelEvent` metadata observer AND the raw signed-event inbound
//!   tap (kinds `[444, 445, 1059]`), return an opaque `*mut MarmotHandle`.
//! - [`nmp_app_chirp_marmot_snapshot`] — JSON snapshot
//!   (`groups` / `pending_welcomes` / `key_package`).
//! - [`nmp_app_chirp_marmot_group_messages`] — newest-N decrypted messages
//!   for one group (hex id), JSON array.
//! - [`nmp_app_chirp_marmot_dispatch`] — perform one mutating op
//!   (`publish_key_package` / `create_group` / `invite` / `send` /
//!   `leave` / `remove` / `accept_welcome` / `decline_welcome` /
//!   `ingest_signed_event`). Returns `{"ok":true,…}` / `{"ok":false,…}`.
//! - [`nmp_app_chirp_marmot_string_free`] — companion deallocator.
//! - [`nmp_app_chirp_marmot_unregister`] — drop both kernel
//!   registrations (lossy observer + raw tap) + free the handle.
//!   Idempotent.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-marmot`; this crate is the
//!   composition point (ADR-0009, kernel boundary). No MLS / MDK type
//!   crosses this FFI — `group_id` is hex, errors are strings, exactly the
//!   typed translation layer `nmp-marmot` asked a consumer to provide.
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, JSON parse / serialize failures, poisoned mutexes, and
//!   `MarmotService` errors all degrade to `null` / `{"ok":false}` rather
//!   than panicking across the FFI.
//!
//! ## Outbound relay seam — CLOSED
//!
//! Where an op produces events that must reach relays
//! (`publish_key_package`'s kind:30443/443, `create_group` /
//! `invite`'s kind:445 commit + kind:1059 gift-wraps, `send`'s kind:445,
//! `accept_welcome`'s post-join kind:445 self-update), this crate performs
//! the `MarmotService` op and then publishes the signed events INTERNALLY
//! via [`nmp_marmot::projection::publish`] (the `nmp-core`
//! `nmp_app_publish_signed_event*` kernel capabilities, called against the
//! retained `*mut NmpApp`). There is NO Swift relay path — that hook never
//! existed (see `MarmotBridge.swift`). The result still carries the signed
//! event JSON (`event` / `events` / `evolution_event` / `welcome_rumors`)
//! but it is now INFORMATIONAL only; publish already happened
//! (fire-and-forget — success == "submitted to the kernel publish
//! pipeline"). Routing per kind: kind:445 → group-pinned relays
//! (`Explicit`, cache miss → `Auto`); kind:30443/443 → author outbox
//! (`Auto`); kind:1059 gift-wrap → group relays as a documented
//! inbox-routing approximation. The MDK pending-commit is still resolved
//! here (commit eagerly because the events are produced + submitted;
//! clear-on-failure is exposed via the `clear_pending` op).
//!
//! ## Inbound ingest seam — CLOSED
//!
//! `nmp_app_chirp_marmot_register` also registers a raw signed-event tap
//! (`nmp-core` `RawEventObserver`, Rust-trait API) for kinds
//! `[444, 445, 1059]`. The kernel delivers every accepted inbound signed
//! event of those kinds to [`nmp_marmot::projection::tap`], which drives them
//! through the SAME `ops::ingest_signed_event_core` the back-compat
//! `{"op":"ingest_signed_event"}` dispatch op uses — so welcomes /
//! messages received from relays surface in the next snapshot with no
//! Swift involvement (the existing snapshot read is unchanged).
//! `nmp_app_chirp_marmot_unregister` tears down BOTH kernel
//! registrations (the lossy `KernelEvent` metadata observer AND the raw
//! tap; distinct slots / ids). This was the last open seam.

use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use apple_native_keyring_store::protected::Store as AppleStore;
use keyring_core::set_default_store;

use nmp_core::{KernelEventObserverId, NmpApp, RawEventObserver, RawEventObserverId};
use nostr::Keys;
use serde_json::{json, Value};

use nmp_marmot::service::MarmotService;

use nmp_marmot::projection::state::MarmotProjection;
use nmp_marmot::projection::tap::MarmotIngestTap;

/// Default page size for [`nmp_app_chirp_marmot_group_messages`].
const DEFAULT_MESSAGE_PAGE: usize = 200;

/// Keyring coordinates for the production encrypted SQLite DB. Stable
/// strings — the keyring entry is created lazily by `MdkSqliteStorage`.
const KEYRING_SERVICE_ID: &str = "nmp.chirp.marmot";
const KEYRING_DB_KEY_ID: &str = "marmot-mls-db-key";

/// Opaque handle returned by [`nmp_app_chirp_marmot_register`]. Boxed so the
/// address is stable; Swift holds the raw pointer until
/// [`nmp_app_chirp_marmot_unregister`].
pub struct MarmotHandle {
    projection: Arc<MarmotProjection>,
    /// Lossy `KernelEvent` observer (key-package metadata tracker — see
    /// `MarmotProjection::on_kernel_event`). Distinct slot / id from the
    /// raw tap below; both are torn down in `unregister`.
    observer_id: KernelEventObserverId,
    /// Raw signed-event tap (the CLOSED inbound ingest seam — drives
    /// kind:1059/445 into `MarmotService` via the shared core; see
    /// [`nmp_marmot::projection::tap`]). Separate kernel slot from `observer_id`.
    raw_observer_id: RawEventObserverId,
    pub(super) app: *mut NmpApp,
}

// SAFETY: identical rationale to `ChirpHandle` (see `crate::ffi`). The
// auto-derived `!Send`/`!Sync` comes only from `app: *mut NmpApp`; the
// `Arc<MarmotProjection>` is already `Send + Sync`. The earlier comment's
// claim that "Swift drives every call from one serialized bridge dispatch
// queue" is NOT accurate — `KernelHandle` is a plain `final class` with no
// queue. The honest invariant has three layers:
//
//   1. Swift owns this handle and only reaches the FFI entry points below
//      from `@MainActor` types (`KernelModel` / `MarmotStore`), so the
//      handle struct itself is never raced (a documented Swift caller
//      convention, not a type guarantee).
//   2. The `Arc<MarmotProjection>` IS shared across threads — the kernel
//      actor thread runs `MarmotProjection::on_kernel_event` and the raw
//      tap's `on_raw_event` while the Swift main actor calls `snapshot()` /
//      dispatch. Soundness of that sharing comes from `MarmotProjection`'s
//      interior `Mutex<Inner>`, not from this `unsafe impl`.
//   3. The `app` raw pointer is only read (to forward fire-and-forget
//      kernel commands). No use-after-free is possible: `nmp_app_free`'s
//      `NmpApp::Drop` sends `Shutdown` and `join()`s the actor thread
//      before freeing the allocation, and every kernel callback that can
//      reach `app` (`on_kernel_event`, `on_raw_event`) runs INLINE on that
//      actor thread — the join fences them.
//
// CALLER CONTRACT: `nmp_app_free` must not run while a kernel callback that
// reaches this projection is still executing. The in-process Rust-trait
// registration path used here (`register_event_observer` /
// `register_raw_event_observer`) gets that fence from the actor join.
// Calling `nmp_app_chirp_marmot_unregister` before `nmp_app_free` is the
// documented hygiene step; the actor join is the actual fence.
unsafe impl Send for MarmotHandle {}
unsafe impl Sync for MarmotHandle {}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Inner registration logic shared by `nmp_app_chirp_marmot_register` and
/// `nmp_app_chirp_marmot_register_active`. `app` must be non-null and valid.
pub(super) fn register_with_keys(app: *mut NmpApp, keys: Keys, db_path: &str) -> *mut MarmotHandle {
    // Initialize a credential store for `MdkSqliteStorage`. Try the Apple
    // protected store first (required on device); fall back to the in-memory
    // mock store on the simulator where entitlements are missing (-34018).
    //
    // TODO(D7): this multi-step credential-store fallback strategy (Apple
    // protected store → mock store on simulator → retry loop deleting a stale
    // DB) is policy, not glue — it should move into `nmp-marmot` as a
    // `pub fn initialize_credential_store(...)` so every NMP app gets the same
    // behavior. It stays inline here for now ONLY because the obvious home is
    // a protocol crate and the policy is genuinely Apple-platform-coupled
    // (`apple-native-keyring-store`); pushing that crate into `nmp-marmot`
    // (which lib.rs declares the SOLE importer of mdk-core/openmls and is
    // otherwise platform-agnostic) needs a deliberate dependency-graph
    // decision. Once `MarmotService::new` accepts a store-init strategy (or a
    // pre-built `Box<dyn keyring_core::Store>`), this whole block collapses to
    // one `nmp-marmot` call + `return null_mut()` on error.
    let mut use_mock = false;
    if let Ok(store) = AppleStore::new() {
        set_default_store(store);
    } else {
        // D6: mock store construction can fail — never `unwrap()` across the
        // FFI. Degrade to a null handle so Swift sees a clean failure.
        match keyring_core::mock::Store::new() {
            Ok(store) => {
                use_mock = true;
                set_default_store(store);
            }
            Err(_) => return std::ptr::null_mut(),
        }
    }

    let service =
        match MarmotService::new(db_path, KEYRING_SERVICE_ID, KEYRING_DB_KEY_ID, keys.clone()) {
            Ok(s) => s,
            Err(_) => {
                // Stale DB: if the keyring was uninitialized on first creation,
                // the SQLite file exists but has no encryption key entry. Delete
                // the DB (+ WAL/SHM) and retry exactly once.
                let _ = std::fs::remove_file(db_path);
                let _ = std::fs::remove_file(format!("{}-wal", db_path));
                let _ = std::fs::remove_file(format!("{}-shm", db_path));
                match MarmotService::new(
                    db_path,
                    KEYRING_SERVICE_ID,
                    KEYRING_DB_KEY_ID,
                    keys.clone(),
                ) {
                    Ok(s) => s,
                    Err(_) => {
                        // If the Apple store failed because of missing entitlements
                        // on the simulator, the retry above also fails. Switch to
                        // the mock store and try one final time.
                        if !use_mock {
                            // D6: mock store construction can fail — never
                            // `unwrap()` across the FFI; degrade to a null handle.
                            match keyring_core::mock::Store::new() {
                                Ok(store) => set_default_store(store),
                                Err(_) => return std::ptr::null_mut(),
                            }
                            match MarmotService::new(
                                db_path,
                                KEYRING_SERVICE_ID,
                                KEYRING_DB_KEY_ID,
                                keys.clone(),
                            ) {
                                Ok(s) => s,
                                Err(_) => return std::ptr::null_mut(),
                            }
                        } else {
                            return std::ptr::null_mut();
                        }
                    }
                }
            }
        };

    // SAFETY: caller guarantees `app` is non-null and valid.
    let app_ref = unsafe { &*app };
    let projection = Arc::new(MarmotProjection::new(service));
    projection.set_app(app);
    let observer_id = app_ref
        .register_event_observer(Arc::clone(&projection) as Arc<dyn nmp_core::KernelEventObserver>);
    if observer_id.0 == 0 {
        return std::ptr::null_mut(); // poisoned slot — soft fail.
    }

    let tap = Arc::new(MarmotIngestTap::new(Arc::clone(&projection)));
    let raw_observer_id = app_ref.register_raw_event_observer(
        MarmotIngestTap::kind_filter(),
        tap as Arc<dyn RawEventObserver>,
    );
    if raw_observer_id.0 == 0 {
        app_ref.unregister_event_observer(observer_id);
        return std::ptr::null_mut();
    }

    // D7: the gift-wrap inbox subscription (kind:1059 `#p` filter, deterministic
    // id, account scope) is protocol policy — it lives in `nmp-marmot`, not in
    // this glue. The FFI only resolves the concrete pubkey and forwards.
    let pubkey_hex = keys.public_key().to_hex();
    app_ref.push_interest(nmp_marmot::interest::giftwrap_inbox_interest(&pubkey_hex));

    Box::into_raw(Box::new(MarmotHandle {
        projection,
        observer_id,
        raw_observer_id,
        app,
    }))
}

/// Register a Marmot projection against `app`.
///
/// * `app` — the live `NmpApp` (from `nmp_app_new`). MUST outlive the
///   handle. NULL → null handle.
/// * `secret_key_hex` — **signer seam**: the local identity secret as hex
///   or `nsec…`. `MarmotService` signs key-package events and gift-wraps
///   with this key directly until a kernel `Keys` provider exists. NULL or
///   unparuseable → null handle.
/// * `db_dir` — the app-support directory; the DB is created at
///   `<db_dir>/marmot-mls-state.sqlite` (owned by this crate). NULL →
///   null handle.
///
/// Returns a non-null `*mut MarmotHandle` on success; `null` on any
/// failure (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_register(
    app: *mut NmpApp,
    secret_key_hex: *const c_char,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let (Some(sk), Some(dir)) = (c_str_opt(secret_key_hex), c_str_opt(db_dir)) else {
        return std::ptr::null_mut();
    };
    let Ok(keys) = Keys::parse(&sk) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path)
}

/// Register a Marmot projection using the actor-owned active local key.
/// Swift never sees the secret — the key is read from the slot the actor
/// writes after every identity mutation. Returns a non-null handle on
/// success; `null` if no local account is active or `db_dir` is NULL (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_register_active(
    app: *mut NmpApp,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: app is non-null and valid for this call.
    let app_ref = unsafe { &*app };
    let Some(sk) = app_ref.active_local_nsec() else {
        return std::ptr::null_mut();
    };
    let Ok(keys) = Keys::parse(&sk) else {
        return std::ptr::null_mut();
    };
    let Some(dir) = c_str_opt(db_dir) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path)
}

/// JSON snapshot. Null handle / serialize failure → null (D6). Caller owns
/// the returned pointer until [`nmp_app_chirp_marmot_string_free`].
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_snapshot(handle: *mut MarmotHandle) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let snap = handle.projection.snapshot(now_secs());
    to_c_json(&snap)
}

/// Newest-N decrypted messages for the group whose MLS id is
/// `group_id_hex`. JSON array; `[]` on any soft failure (unknown group,
/// poisoned mutex, parse error). Null handle / serialize failure → null.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_group_messages(
    handle: *mut MarmotHandle,
    group_id_hex: *const c_char,
) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let Some(gid_hex) = c_str_opt(group_id_hex) else {
        return to_c_string("[]");
    };
    let rows = handle
        .projection
        .with_inner(|h| {
            nmp_marmot::projection::ops::group_messages(h, &gid_hex, DEFAULT_MESSAGE_PAGE)
        })
        .unwrap_or_default();
    match serde_json::to_string(&rows) {
        Ok(s) => to_c_string(&s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Perform one mutating op. `action_json` is the op envelope (see module
/// rustdoc). Returns `{"ok":true,…}` / `{"ok":false,"error":"…"}`.
/// Null handle / serialize failure → null (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_dispatch(
    handle: *mut MarmotHandle,
    action_json: *const c_char,
) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let Some(action) = c_str_opt(action_json) else {
        return to_c_json(&err("missing action_json"));
    };
    let Ok(v) = serde_json::from_str::<Value>(&action) else {
        return to_c_json(&err("action_json is not valid JSON"));
    };
    let result = handle
        .projection
        .with_inner(|h| nmp_marmot::projection::ops::dispatch(h, &v, now_secs()))
        .unwrap_or_else(|| err("projection mutex poisoned"));
    to_c_json(&result)
}

/// Free a string previously returned by snapshot / group_messages /
/// dispatch. Null is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in one
    // of our string-returning symbols and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Drop the observer registration and free the handle. Idempotent: null is
/// a silent no-op. The handle MUST NOT be used after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_unregister(handle: *mut MarmotHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from
    // `nmp_app_chirp_marmot_register` and has not already been freed.
    let boxed = unsafe { Box::from_raw(handle) };
    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as register.
        let app_ref = unsafe { &*boxed.app };
        // Drop both kernel registrations (distinct slots): the lossy
        // metadata observer AND the raw inbound-ingest tap. Both are
        // idempotent no-ops for unknown ids (D6). Dropping the raw tap
        // releases the kernel's `Arc<dyn RawEventObserver>`, which in turn
        // releases the tap's `Arc<MarmotProjection>` clone — no
        // use-after-free of `app` (it is read only here, then `boxed`
        // drops).
        app_ref.unregister_event_observer(boxed.observer_id);
        app_ref.unregister_raw_event_observer(boxed.raw_observer_id);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

pub(super) fn c_str_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn to_c_json<T: serde::Serialize>(v: &T) -> *mut c_char {
    match serde_json::to_string(v) {
        Ok(s) => to_c_string(&s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `{"ok":false,"error":"…"}`
pub(crate) fn err(msg: &str) -> Value {
    json!({ "ok": false, "error": msg })
}

#[cfg(test)]
mod tests;
