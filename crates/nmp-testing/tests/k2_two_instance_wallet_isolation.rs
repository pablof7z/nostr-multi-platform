//! K2 rung 5.2 oracle — **two-instance wallet isolation** (ADR-0052 §D1/D2).
//!
//! Two full Chirp `NmpApp` instances are constructed in ONE process, each
//! connected to a DIFFERENT NWC wallet (a distinct wallet service pubkey).
//! Each app dispatches its own `nmp.wallet.connect`; the test then asserts
//! each app's `"wallet"` snapshot projection reports **its own** wallet
//! pubkey — never the other app's. This is the falsifiable proof that the
//! wallet runtime is instance-scoped, not process-global.
//!
//! # Why this fails before rung 5.2
//!
//! Before this rung the wallet runtime lives in a process-global
//! `ACTIVE_WALLET_RUNTIME: OnceLock<WalletRuntimeHandle>`
//! (`nmp-nip47/src/runtime.rs`). The composition root
//! (`apps/chirp/.../wallet_runtime.rs`) installs it via
//! `install_wallet_runtime`, which is **first-writer-wins**: the FIRST app to
//! register claims the `OnceLock`; the SECOND app's install is a silent
//! no-op. Both apps' `ActionModule::execute` then read the SAME (first app's)
//! runtime through `active_wallet_runtime()`. The second app's connect
//! therefore mutates the first app's runtime — crosstalk — and the second
//! app's own `"wallet"` projection never reflects its distinct wallet pubkey.
//!
//! After rung 5.2 each `WalletConnectModule` value owns its own
//! `Arc<WalletRuntimeHandle>` (captured at composition time), so the two apps
//! are fully independent and the assertions below hold.
//!
//! The connect path used here is **relay-free**: `wallet_connect`
//! synchronously writes `WalletConnection.wallet_pubkey_hex` into the status
//! slot and calls `sync_wallet_status` before any relay round-trip, so the
//! `"wallet"` projection carries the connected wallet's pubkey immediately
//! after the actor processes the command — no `nak serve`, no network.

#[path = "zap_e2e_common/mod.rs"]
mod zap_e2e_common;

use std::ffi::CString;
use std::time::{Duration, Instant};

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus,
};
use nmp_ffi::{
    nmp_app_free, nmp_app_new,
    nmp_app_signin_nsec, nmp_app_start, NmpApp,
};
use nostr::{Keys, SecretKey, ToBech32};

use zap_e2e_common::{install_emit_signal, nwc_uri, wait_for_projection};

/// Build a full Chirp app (the same composition the iOS shell ships, which
/// registers the NIP-47 wallet stack under the `wallet` feature) and sign in a
/// fresh local key so the active account exists.
fn build_chirp_app() -> (*mut NmpApp, *mut ChirpHandle) {
    let app = nmp_app_new();
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "chirp register must succeed"
    );
    assert!(!handle.is_null(), "chirp register must return a handle");
    let nsec = Keys::generate().secret_key().to_bech32().expect("nsec");
    let nsec_c = CString::new(nsec).expect("nsec NUL-free");
    nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);
    (app, handle)
}

/// Dispatch `nmp.wallet.connect` for `app` against a wallet whose service
/// pubkey is `wallet_pubkey_hex`, pointed at a (never-dialed) relay URL.
///
/// ADR-0064 / Cut-B (#1756): uses the typed byte doorway
/// (`nmp_app_chirp::dispatch_bytes::dispatch_action_bytes_for`) — the
/// deleted JSON doorway (`nmp_app_dispatch_action`) and the deleted bespoke
/// `nmp_app_wallet_connect` symbol (D11) are both gone.
fn connect_wallet(app: *mut NmpApp, wallet_pubkey_hex: &str) {
    // A throwaway client secret — only its syntactic validity matters; the
    // relay is never dialed in this test, so no handshake occurs.
    let client_secret_hex = SecretKey::generate().to_secret_hex();
    let uri = nwc_uri(
        wallet_pubkey_hex,
        "ws://127.0.0.1:1/never-dialed",
        &client_secret_hex,
    );
    let action_json = serde_json::to_string(&serde_json::json!({
        "Connect": { "uri": uri }
    }))
    .expect("connect action JSON must serialize");
    // Result is Ok(correlation_id) or Err(message); we don't wait for the
    // action terminal here — the projection-poll loop in
    // `projected_wallet_pubkey` is the synchronisation point.
    let _ = nmp_app_chirp::dispatch_bytes::dispatch_action_bytes_for(
        app,
        "nmp.wallet.connect",
        &action_json,
    );
}

/// The connected wallet pubkey reported by an app's `"wallet"` projection,
/// or `None` if the projection is absent / has no connection yet.
fn projected_wallet_pubkey(
    app: *mut NmpApp,
    rx: &std::sync::mpsc::Receiver<()>,
    deadline: Instant,
) -> Option<String> {
    wait_for_projection(app, rx, "wallet", deadline, |v| {
        v.get("wallet_pubkey_hex")
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.is_empty())
    })
    .and_then(|v| {
        v.get("wallet_pubkey_hex")
            .and_then(|s| s.as_str())
            .map(str::to_owned)
    })
}

#[test]
fn two_apps_two_wallets_no_crosstalk() {
    // Two distinct wallet service identities.
    let wallet_a = Keys::generate().public_key().to_hex();
    let wallet_b = Keys::generate().public_key().to_hex();
    assert_ne!(wallet_a, wallet_b, "wallet identities must differ");

    let (app_a, handle_a) = build_chirp_app();
    let (app_b, handle_b) = build_chirp_app();
    let rx_a = install_emit_signal(app_a);
    let rx_b = install_emit_signal(app_b);
    nmp_app_start(app_a, 256, 4);
    nmp_app_start(app_b, 256, 4);

    // Each app connects to its OWN wallet.
    connect_wallet(app_a, &wallet_a);
    connect_wallet(app_b, &wallet_b);

    let deadline = Instant::now() + Duration::from_secs(10);
    let seen_a = projected_wallet_pubkey(app_a, &rx_a, deadline);
    let seen_b = projected_wallet_pubkey(app_b, &rx_b, deadline);

    // Tear down before asserting so a failure still frees both apps.
    nmp_app_chirp_unregister(handle_a);
    nmp_app_chirp_unregister(handle_b);
    nmp_app_free(app_a);
    nmp_app_free(app_b);

    let seen_a = seen_a.expect("app A's wallet projection must report a connected wallet pubkey");
    let seen_b = seen_b.expect("app B's wallet projection must report a connected wallet pubkey");

    assert_eq!(
        seen_a, wallet_a,
        "app A must see ITS OWN wallet pubkey, got {seen_a} (expected {wallet_a}) — \
         crosstalk via a shared process-global wallet runtime",
    );
    assert_eq!(
        seen_b, wallet_b,
        "app B must see ITS OWN wallet pubkey, got {seen_b} (expected {wallet_b}) — \
         crosstalk via a shared process-global wallet runtime",
    );
    assert_ne!(
        seen_a, seen_b,
        "the two apps must report DIFFERENT wallets — a shared global makes them identical",
    );
}
