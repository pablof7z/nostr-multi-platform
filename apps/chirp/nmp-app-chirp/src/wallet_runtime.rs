//! Chirp's NIP-47 wallet composition entry point.
//!
//! The reusable wallet wiring (runtime install, relay-text interceptor, the
//! `nmp.wallet.{connect,disconnect,pay_invoice}` action modules, and the
//! generic + typed `"wallet"` snapshot projections) lives in
//! [`nmp_nip47::register_wallet`] — it is app-neutral and reused via the
//! `NmpAppBuilder::with_wallet` typed builder step (V-95 / issue #619). Chirp
//! drives the raw C-ABI `NmpApp` registration path, so it calls the reusable
//! function directly here, reading the durable storage path off the app.

use nmp_ffi::NmpApp;

/// Register the NIP-47 wallet stack on `app`. Called by
/// `nmp_app_chirp_register` when the `wallet` feature is on.
///
/// Thin delegate to [`nmp_nip47::register_wallet`]: the only Chirp-specific
/// input is the durable payment-store storage path, read off the un-started
/// app. `NmpApp` implements `AppHost`, so the reusable wiring binds every
/// registration against it.
pub(crate) fn register_nip47_wallet(app: &mut NmpApp) {
    let storage_path = app.storage_path_for_start();
    nmp_nip47::register_wallet(app, storage_path);
}
