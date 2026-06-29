//! T122 — NIP-46 bunker signing through the production native runtime seam.
//!
//! BLOCKER 1 of the PR-B2 review: the other NIP-46 integration tests drive
//! `init_bunker` / `init_nostrconnect` directly (via `broker_adapter.rs`) or
//! exercise the lower-level runtime — they never go through the real FFI init
//! wiring. This test closes that gap: it constructs a real [`NmpApp`], calls the
//! actual `NmpApp::init_signer_broker` config path (which calls `register_nip46`
//! and installs the per-app bunker hook), starts the actor, and drives the
//! bunker connect through the runtime add-signer path against a real
//! [`MockBunkerRelay`].
//!
//! ## What this proves that the lower-level tests do NOT
//!
//! 1. `NmpApp::init_signer_broker` actually installs the interceptor + connected
//!    hook + per-app bunker hook on the `NmpApp` (init-wiring + app-slot storage).
//! 2. A **second** `NmpApp::init_signer_broker` call is an idempotent no-op
//!    (first-writer-wins; no duplicate hooks) — returns `Ok` (0).
//! 3. Bunker sign-in routes through the actor → `start_bunker_handshake`
//!    → the real installed bunker hook → `init_bunker` → the actor relay lane
//!    dials the mock → the registered `Nip46Interceptor` processes inbound
//!    frames → `SignerReady` builds a real `Nip46Signer` and `AddSigner` lands
//!    the active account (observed via the typed `active_account` sidecar).
//!
//! ## Assertions
//!
//! - `NmpApp::init_signer_broker` returns `Ok` (0) on both the first and second call.
//! - The `active_account` typed sidecar carries the bunker user's pubkey.
//! - The mock observed `connect` and `get_public_key`.

mod common;

use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nmp_core::decode_snapshot_typed_projections;
use nmp_core::typed_projections::{decode_active_account, ACTIVE_ACCOUNT_SCHEMA_ID};
use nostr::Keys;

use crate::common::mock_bunker_relay::MockBunkerRelay;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

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
fn bunker_signin_through_production_native_runtime_seam() {
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
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let last_frame: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<()>();
    let ctx = Box::into_raw(Box::new(EmitCtx {
        tx,
        last_frame: Arc::clone(&last_frame),
    }));
    let ctx_usize = ctx as usize;
    unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(move |bytes: &[u8]| {
        on_emit(ctx_usize as *mut c_void, bytes.as_ptr(), bytes.len());
    })));

    // ── Production init seam: NmpApp::init_signer_broker ──────────────────────
    // First call installs the interceptor + connected hook + per-app bunker hook.
    let status1 = unsafe { &*app }.init_signer_broker().code();
    assert_eq!(status1, 0, "first init_signer_broker must return Ok (0)");

    // Second call is an idempotent no-op (first-writer-wins): no duplicate hooks.
    let status2 = unsafe { &*app }.init_signer_broker().code();
    assert_eq!(
        status2, 0,
        "second init_signer_broker must be an idempotent no-op returning Ok (0)"
    );

    // ── Start the actor ───────────────────────────────────────────────────────
    unsafe { &*app }.start_runtime(256, 30);

    // ── Drive the bunker connect through the runtime front door ───────────────
    unsafe { &*app }.add_signer(nmp_core::SignerSource::BunkerUri(bunker_uri), true);

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
         (proves AddSigner through the production native runtime seam)"
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

    unsafe { drop(Box::from_raw(app)) };
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
