//! `nmp-app-notes` — the second-app **stateful** spike.
//!
//! A minimal NIP-01 note client that:
//!
//! * Signs in via nsec (`nmp_app_signin_nsec`) or NIP-46 bunker
//!   (`nmp_signer_broker_init` + `nmp_app_signin_bunker` /
//!   `nmp_app_nostrconnect_uri`).
//! * Reads kind:1 notes through `nmp_app_register_raw_event_observer`
//!   with the `kinds_json = "[1]"` filter.
//! * Publishes kind:1 notes through the single
//!   `nmp_app_dispatch_action(app, "nmp.publish", json)` door, with a
//!   `PublishNote` action body (target = `"Auto"`).
//!
//! # Why this crate exists
//!
//! `apps/longform/nmp-app-longform` proved the framework thesis for the
//! **read-only** path (kind:30023 article reader on substrate primitives,
//! ~150 LOC Rust). This crate proves the same thesis for the **stateful
//! write path** by composing ONLY the generic substrate seams that
//! `nmp-core` + `nmp-ffi` already export.
//!
//! # Shape
//!
//! Pure composition. The crate adds **zero** new `#[no_mangle]` C-ABI
//! protocol symbols. The only `extern "C"` symbol introduced here is
//! `nmp_app_notes_init`, which is an app-registration marker, not a new
//! protocol seam. Everything Swift needs is already exported by:
//!
//! * `nmp_core::*` (via the `android-ffi` feature, see `Cargo.toml`).
//! * `nmp_ffi::*`.
//!
//! The `pub use` statements below are what cause rustc to pull each symbol
//! body into the CGU that ends up inside `libnmp_app_notes.a` — without
//! the Rust-path reference the `#[no_mangle]` symbols stay `U` (undefined)
//! in the archive and the iOS link step would fail.
//!
//! # D0 — no Chirp deps, no protocol-crate deps
//!
//! `Cargo.toml` depends on `nmp-core` + `nmp-ffi` only. No
//! `nmp-nip01`, no `nmp-app-chirp`, no `nmp-marmot`. The crate name appears
//! nowhere in any social/NIP-protocol Cargo file.

// Re-export every C-ABI symbol the iOS Notes shell needs. This is what
// causes the linker to embed the symbol bodies inside `libnmp_app_notes.a`.
// `nmp-core`'s `lib.rs` already re-exports the full surface (native +
// android-ffi deltas); a glob pulls them all in one statement.
//
// `#[allow(unused_imports)]` — the symbols are consumed by the C linker on
// the iOS side, not by any Rust code in this crate; without the allow,
// `cargo check` warns about each re-export.
#[allow(unused_imports)]
pub use nmp_core::*;
#[allow(unused_imports)]
pub use nmp_ffi::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_broker_free_string,
    nmp_signer_broker_init,
};

/// App-registration marker. Called once by the Swift shell after
/// `nmp_app_new()` and before `nmp_app_start()`.
///
/// The Notes spike registers **no** custom projections from Rust: the
/// iOS-side raw event observer (`nmp_app_register_raw_event_observer` with
/// `kinds_json = "[1]"`) is enough to drive the timeline view. Keeping
/// this body empty is intentional — it documents the boundary that any
/// future stateful spike could carry custom projection state behind without
/// changing the FFI contract.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new`. NULL is
/// tolerated (silent no-op, matching every other `nmp_app_*` D6 contract).
#[no_mangle]
pub extern "C" fn nmp_app_notes_init(_app: *mut nmp_ffi::NmpApp) {
    // No custom projections needed for the spike.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_tolerates_null_app() {
        // D6 contract: every `nmp_app_*` symbol degrades silently on NULL.
        nmp_app_notes_init(std::ptr::null_mut());
    }

    #[test]
    fn init_with_real_app_is_idempotent() {
        // The marker is empty, so repeat calls must be safe. Allocates a
        // real `NmpApp` to make sure the call site type-checks against the
        // re-exported `NmpApp` from `nmp_core`. Step 11 final moved the
        // C-ABI symbols to `nmp-ffi`; this test reaches them through that
        // crate's public Rust path.
        let app = nmp_ffi::nmp_app_new();
        nmp_app_notes_init(app);
        nmp_app_notes_init(app);
        nmp_ffi::nmp_app_free(app);
    }
}
