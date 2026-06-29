//! NIP-55 external-signer UniFFI methods — M14-C2.
//!
//! Mirrors `nmp-ffi/src/external_signer.rs` for the three external-signer
//! symbols: `nmp_external_signer_init`, `nmp_app_signin_nip55`,
//! `nmp_app_deliver_external_signer_response`.
//!
//! Each method calls the SAME underlying `nmp_native_runtime::NmpApp` method
//! the C-ABI wrapper calls. No logic is duplicated.
//!
//! ## Feature gate
//!
//! This module is compiled only when the `external-signer` feature is active.
//! The `native` default feature enables it, so the generated Swift/Kotlin
//! bindings always include these methods.
//!
//! ## Doctrine
//!
//! * D7 — the host fires what Rust built and reports raw results; Rust owns
//!   all policy.
//! * D6 — malformed responses degrade gracefully; no panic across the FFI.

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Initialise the NIP-55 external-signer capability transport.
    ///
    /// Must be called before `signin_nip55` to wire up the
    /// `external_signer` capability namespace. Idempotent — a second call is
    /// a no-op.
    ///
    /// Mirrors `nmp_external_signer_init`.
    pub fn init_external_signer(&self) {
        self.inner.init_external_signer();
    }

    /// Initiate a NIP-55 (external-signer, e.g. Amber) sign-in flow.
    ///
    /// `signer_package` — optional opaque package hint forwarded to the
    /// registered capability callback (e.g. the Amber app's package name on
    /// Android). `None` means no hint.
    ///
    /// Mirrors `nmp_app_signin_nip55`.
    pub fn signin_nip55(&self, signer_package: Option<String>) {
        self.inner.signin_nip55(signer_package);
    }

    /// Deliver a raw NIP-55 response JSON from the host capability bridge
    /// back into the Rust signer driver.
    ///
    /// Called by the host after the external signer (e.g. Amber) returns a
    /// response Intent / resolver result. The JSON is forwarded opaquely to
    /// the `Nip55Driver` inside `nmp-signers`.
    ///
    /// D6: malformed JSON degrades to a timeout on the pending signer
    /// operation — never a panic.
    ///
    /// Mirrors `nmp_app_deliver_external_signer_response`.
    pub fn deliver_external_signer_response(&self, response_json: String) {
        self.inner.deliver_external_signer_response(&response_json);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parity with C-ABI `nmp_external_signer_init`:
    /// calling on a fresh app must not panic (D6 — idempotent pre-start config).
    #[test]
    fn parity_init_external_signer_no_panic() {
        let app = crate::NmpApp::new();
        // D6: must not panic on a fresh app without capability callback.
        app.init_external_signer();
    }

    /// Parity with C-ABI `nmp_external_signer_init` idempotent second call:
    /// calling twice must not panic (first-writer-wins idempotence).
    #[test]
    fn parity_init_external_signer_idempotent() {
        let app = crate::NmpApp::new();
        app.init_external_signer();
        app.init_external_signer(); // second call must be a no-op
    }

    /// Parity with C-ABI `nmp_app_signin_nip55(NULL package)`:
    /// `signin_nip55(None)` must not panic (D6 — degrades gracefully when no
    /// capability handler is registered).
    #[test]
    fn parity_signin_nip55_no_package_no_panic() {
        let app = crate::NmpApp::new();
        app.signin_nip55(None);
    }

    /// Parity with C-ABI `nmp_app_deliver_external_signer_response`:
    /// delivering malformed JSON must not panic (D6 — degrades to timeout,
    /// not a trap). Mirrors the C-ABI path where any invalid JSON is simply
    /// passed to the driver, which handles the error internally.
    #[test]
    fn parity_deliver_external_signer_response_malformed_no_panic() {
        let app = crate::NmpApp::new();
        // D6: malformed JSON must not panic.
        app.deliver_external_signer_response("not valid json".to_string());
    }
}
