//! T122 — NIP-46 bunker signing through the **production C-ABI seam** (#2119 PR-B2).
//!
//! BLOCKER 1 of the PR-B2 review: the other NIP-46 integration tests drive
//! `init_bunker` / `init_nostrconnect` directly (via `broker_adapter.rs`) or
//! exercise the lower-level runtime — they never go through the real FFI init
//! wiring. This test closes that gap: it constructs a real [`NmpApp`], calls the
//! actual `nmp_signer_broker_init` C symbol (which calls `register_nip46` and
//! installs the per-app bunker hook), starts the actor, and drives the bunker
//! connect through `nmp_app_signin_bunker` against a real [`MockBunkerRelay`].
//!
//! ## What this proves that the lower-level tests do NOT
//!
//! 1. `nmp_signer_broker_init` actually installs the interceptor + connected
//!    hook + per-app bunker hook on the `NmpApp` (init-wiring + app-slot storage).
//! 2. A **second** `nmp_signer_broker_init` call is an idempotent no-op
//!    (first-writer-wins; no duplicate hooks) — returns `Ok` (0).
//! 3. `nmp_app_signin_bunker` routes through the actor → `start_bunker_handshake`
//!    → the real installed bunker hook → `init_bunker` → the actor relay lane
//!    dials the mock → the registered `Nip46Interceptor` processes inbound
//!    frames → `SignerReady` builds a real `Nip46Signer` and `AddSigner` lands
//!    the active account (observed via the typed `active_account` sidecar).
//! 4. A subsequent sign (`nmp_app_sign_event_for_return`) routes the
//!    `sign_event` RPC back out through the actor relay lane to the mock, whose
//!    encrypted reply is decoded by the runtime and resolves the parked op —
//!    yielding a **schnorr-valid** signed event.
//!
//! ## Assertions
//!
//! - `nmp_signer_broker_init` returns `Ok` (0) on both the first and second call.
//! - The `active_account` typed sidecar carries the bunker user's pubkey.
//! - The mock observed `connect`, `get_public_key`, and `sign_event`.
//! - The returned signed event re-verifies with `nostr::Event::verify`.

mod common;

use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nmp_core::decode_snapshot_typed_projections;
use nmp_core::typed_projections::{
    decode_active_account, decode_signed_events, ACTIVE_ACCOUNT_SCHEMA_ID, SIGNED_EVENTS_SCHEMA_ID,
};
use nmp_ffi::{
    nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_signin_bunker, nmp_app_start,
    nmp_free_string, nmp_signer_broker_init, NmpApp,
};
use nostr::util::JsonUtil;
use nostr::{Event, Keys};

use crate::common::mock_bunker_relay::MockBunkerRelay;

// `nmp_app_sign_event_for_return` is a `#[no_mangle] extern "C"` symbol in
// nmp-ffi but is not re-exported on the Rust path; reach it through the C ABI
// (the exact seam the iOS / Android shells use for Blossom-auth signing).
// `NmpApp` is an opaque handle passed only as a pointer — the `improper_ctypes`
// lint about its layout is moot (the Swift/Kotlin side treats it as `void *`).
#[allow(improper_ctypes)]
extern "C" {
    fn nmp_app_sign_event_for_return(
        app: *mut NmpApp,
        account_pubkey_hex: *const c_char,
        unsigned_json: *const c_char,
    ) -> *mut c_char;
}

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const SIGN_TIMEOUT: Duration = Duration::from_secs(15);

/// Emit-callback context: signals the test thread and caches the last frame.
struct EmitCtx {
    tx: Sender<()>,
    last_frame: Arc<Mutex<Option<Vec<u8>>>>,
}

/// Kernel emit callback (runs on the actor thread). Caches the frame bytes for
/// typed-sidecar decode and wakes the test thread.
extern "C" fn on_emit(context: *mut c_void, ptr: *const u8, len: usize) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the leaked `EmitCtx` box installed below; it outlives
    // every callback (never freed during the test).
    let ctx = unsafe { &*(context as *const EmitCtx) };
    if !ptr.is_null() && len > 0 {
        // SAFETY: `ptr`/`len` reference the kernel frame buffer, valid for the
        // duration of this call.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        if let Ok(mut g) = ctx.last_frame.lock() {
            *g = Some(bytes);
        }
    }
    let _ = ctx.tx.send(());
}

#[test]
fn bunker_signin_and_sign_through_production_ffi_seam() {
    // Plain ws:// mock — no TLS — but install the ring provider defensively in
    // case the actor relay worker initialises a rustls client config.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // ── Mock relay + bunker URI ───────────────────────────────────────────────
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let user_pubkey_hex = user_keys.public_key().to_hex();
    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn on 127.0.0.1");
    let bunker_uri = format!(
        "bunker://{}?relay={}",
        bunker_keys.public_key().to_hex(),
        mock.ws_url(),
    );

    // ── Construct the app + install emit signal ───────────────────────────────
    let app = nmp_app_new();
    let last_frame: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<()>();
    let ctx = Box::into_raw(Box::new(EmitCtx {
        tx,
        last_frame: Arc::clone(&last_frame),
    }));
    nmp_app_set_update_callback(app, ctx as *mut c_void, Some(on_emit));

    // ── Production init seam: nmp_signer_broker_init ──────────────────────────
    // First call installs the interceptor + connected hook + per-app bunker hook.
    let status1 = nmp_signer_broker_init(app);
    assert_eq!(status1, 0, "first nmp_signer_broker_init must return Ok (0)");

    // Second call is an idempotent no-op (first-writer-wins): no duplicate hooks.
    let status2 = nmp_signer_broker_init(app);
    assert_eq!(
        status2, 0,
        "second nmp_signer_broker_init must be an idempotent no-op returning Ok (0)"
    );

    // ── Start the actor ───────────────────────────────────────────────────────
    nmp_app_start(app, 256, 30);

    // ── Drive the bunker connect through the REAL FFI front door ──────────────
    let uri_c = CString::new(bunker_uri).expect("uri NUL-free");
    nmp_app_signin_bunker(app, uri_c.as_ptr(), 1);

    // ── Wait for AddSigner via the typed active_account sidecar ───────────────
    // Reaching this state means: bunker hook fired → init_bunker effects → actor
    // relay lane dialed the mock → interceptor processed connect/get_public_key →
    // SignerReady → Nip46Signer built → AddSigner made the account active.
    let active = wait_for_active_account(
        &rx,
        &last_frame,
        &user_pubkey_hex,
        Instant::now() + HANDSHAKE_TIMEOUT,
    );
    assert!(
        active,
        "active_account sidecar must carry the bunker user pubkey after handshake \
         (proves AddSigner through the production FFI seam)"
    );

    // The mock must have observed the handshake RPCs.
    let methods = mock.observed_methods();
    assert!(
        methods.iter().any(|m| m == "connect"),
        "mock must have seen `connect`, got {methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "get_public_key"),
        "mock must have seen `get_public_key`, got {methods:?}"
    );

    // ── Drive a sign through the production FFI seam ──────────────────────────
    let draft = r#"{"kind":1,"content":"t122 production-path bunker sign","tags":[],"created_at":0}"#;
    let empty = CString::new("").unwrap();
    let draft_c = CString::new(draft).unwrap();
    let cid_ptr = unsafe { nmp_app_sign_event_for_return(app, empty.as_ptr(), draft_c.as_ptr()) };
    assert!(!cid_ptr.is_null(), "a correlation_id C string is returned");
    let correlation_id = unsafe { CStr::from_ptr(cid_ptr) }
        .to_str()
        .expect("utf-8 correlation_id")
        .to_string();
    nmp_free_string(cid_ptr);
    assert!(!correlation_id.is_empty(), "correlation_id is non-empty");

    // ── Wait for the signed event to surface in the signed_events sidecar ─────
    let signed_json = wait_for_signed_event(
        &rx,
        &last_frame,
        &correlation_id,
        Instant::now() + SIGN_TIMEOUT,
    )
    .expect("signed event must surface — bunker sign round-trip via production FFI seam");

    // The mock must have observed the `sign_event` RPC.
    let methods_after = mock.observed_methods();
    assert!(
        methods_after.iter().any(|m| m == "sign_event"),
        "mock must have seen `sign_event` after sign call, got {methods_after:?}"
    );

    // ── Schnorr-verify the signed event ───────────────────────────────────────
    let event = Event::from_json(&signed_json).expect("signed_json parses as a nostr Event");
    event
        .verify()
        .expect("the bunker-signed event must have a schnorr-valid id + signature");
    assert_eq!(event.kind.as_u16(), 1, "kind:1 was signed");
    assert_eq!(
        event.pubkey.to_hex(),
        user_pubkey_hex,
        "the signed event carries the bunker user's pubkey (not the bunker pubkey)"
    );

    nmp_app_free(app);
    // mock + leaked EmitCtx box drop/leak at process exit (test lifetime).
}

/// Block (D8: OS-event wait on the emit channel) until the typed `active_account`
/// sidecar carries `expected_pubkey_hex`, or the deadline passes.
fn wait_for_active_account(
    rx: &Receiver<()>,
    last_frame: &Arc<Mutex<Option<Vec<u8>>>>,
    expected_pubkey_hex: &str,
    deadline: Instant,
) -> bool {
    loop {
        if active_account_matches(last_frame, expected_pubkey_hex) {
            return true;
        }
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(r) => r,
            None => return active_account_matches(last_frame, expected_pubkey_hex),
        };
        if rx.recv_timeout(remaining.min(Duration::from_millis(250))).is_err()
            && Instant::now() >= deadline
        {
            return active_account_matches(last_frame, expected_pubkey_hex);
        }
    }
}

fn active_account_matches(
    last_frame: &Arc<Mutex<Option<Vec<u8>>>>,
    expected_pubkey_hex: &str,
) -> bool {
    let Some(bytes) = last_frame.lock().ok().and_then(|g| g.clone()) else {
        return false;
    };
    let Ok(typed) = decode_snapshot_typed_projections(&bytes) else {
        return false;
    };
    typed
        .iter()
        .find(|t| t.key == ACTIVE_ACCOUNT_SCHEMA_ID)
        .and_then(|t| decode_active_account(&t.payload).ok())
        .and_then(|m| m.pubkey)
        .is_some_and(|pk| pk == expected_pubkey_hex)
}

/// Block until the typed `signed_events` sidecar carries a successful entry for
/// `correlation_id`, returning its `signed_json`. `None` on deadline.
fn wait_for_signed_event(
    rx: &Receiver<()>,
    last_frame: &Arc<Mutex<Option<Vec<u8>>>>,
    correlation_id: &str,
    deadline: Instant,
) -> Option<String> {
    loop {
        if let Some(json) = signed_event_for(last_frame, correlation_id) {
            return Some(json);
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if rx.recv_timeout(remaining.min(Duration::from_millis(250))).is_err()
            && Instant::now() >= deadline
        {
            return signed_event_for(last_frame, correlation_id);
        }
    }
}

fn signed_event_for(
    last_frame: &Arc<Mutex<Option<Vec<u8>>>>,
    correlation_id: &str,
) -> Option<String> {
    let bytes = last_frame.lock().ok().and_then(|g| g.clone())?;
    let typed = decode_snapshot_typed_projections(&bytes).ok()?;
    let sidecar = typed.iter().find(|t| t.key == SIGNED_EVENTS_SCHEMA_ID)?;
    let model = decode_signed_events(&sidecar.payload).ok()?;
    let (_, row) = model
        .entries
        .into_iter()
        .find(|(key, _)| key == correlation_id)?;
    if row.ok {
        row.signed_json
    } else {
        None
    }
}
