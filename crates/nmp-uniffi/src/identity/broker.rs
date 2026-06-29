//! NIP-46 signer-broker UniFFI methods — M14-C2.
//!
//! Mirrors `nmp-ffi/src/signer_broker.rs` for the three broker symbols:
//! `nmp_signer_broker_init`, `nmp_app_cancel_bunker_handshake`,
//! `nmp_app_nostrconnect_uri`.
//!
//! Each method calls the SAME underlying `nmp_native_runtime::NmpApp` method
//! the C-ABI wrapper calls. No logic is duplicated.
//!
//! ## Feature gate
//!
//! This module is compiled only when the `signer-broker` feature is active.
//! The `native` default feature enables it, so the generated Swift/Kotlin
//! bindings always include these methods.
//!
//! ## Error mapping
//!
//! `init_signer_broker` returns `Result<(), NmpError>` instead of the C-ABI
//! raw `u32` `NmpConfigStatus` code. Mapping:
//! - `NmpConfigStatus::Ok`           → `Ok(())`
//! - `NmpConfigStatus::AlreadyStarted` → `Err(NmpError::AlreadyStarted)`
//! - `NmpConfigStatus::NullApp`      → cannot occur (UniFFI `self` is non-null `Arc`)
//! - `NmpConfigStatus::Unavailable`  → treated as Ok (no hook slot to fail; init is
//!                                      idempotent first-writer-wins per the impl)

use nmp_native_runtime::NmpConfigStatus;

use crate::NmpError;
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Initialise the NIP-46 actor-lane runtime for bunker / nostrconnect
    /// sign-in.
    ///
    /// Must be called before `signin_bunker` / `nostrconnect_uri` can
    /// complete a handshake. Idempotent (first-writer-wins): a second call
    /// before start is a no-op returning `Ok(())`.
    ///
    /// Returns `Err(NmpError::AlreadyStarted)` if called after the runtime
    /// has started — mirrors the C-ABI `NmpConfigStatus::AlreadyStarted`
    /// (u32 code 2) return value of `nmp_signer_broker_init`.
    pub fn init_signer_broker(&self) -> Result<(), NmpError> {
        match self.inner.init_signer_broker() {
            NmpConfigStatus::Ok => Ok(()),
            NmpConfigStatus::AlreadyStarted => Err(NmpError::AlreadyStarted),
            // NullApp cannot occur (Arc is non-null).
            // Unavailable is treated as Ok — broker is idempotent.
            NmpConfigStatus::NullApp | NmpConfigStatus::Unavailable => Ok(()),
        }
    }

    /// Cancel an in-flight NIP-46 bunker handshake, if any.
    ///
    /// No-op when no handshake is in progress. Mirrors
    /// `nmp_app_cancel_bunker_handshake`.
    pub fn cancel_bunker_handshake(&self) {
        self.inner.cancel_bunker_handshake();
    }

    /// Generate a fresh `nostrconnect://` URI for app-initiated NIP-46 flows.
    ///
    /// Returns `None` when called before `init_signer_broker` or when relay
    /// selection fails. The optional `callback_scheme` is platform metadata
    /// appended as `&callback=<encoded>` — it does NOT affect relay selection
    /// (D3: relay selection is Rust-owned).
    ///
    /// Mirrors `nmp_app_nostrconnect_uri`.
    pub fn nostrconnect_uri(&self, callback_scheme: Option<String>) -> Option<String> {
        self.inner
            .nostrconnect_uri(callback_scheme.as_deref().filter(|s| !s.is_empty()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parity with C-ABI `nmp_signer_broker_init` before start:
    /// returns `Ok(())` on a fresh (not-yet-started) app, mirroring
    /// `NmpConfigStatus::Ok` (u32 0).
    #[test]
    fn parity_init_signer_broker_before_start_returns_ok() {
        let app = crate::NmpApp::new();
        let result = app.init_signer_broker();
        assert!(
            result.is_ok(),
            "init_signer_broker on fresh app must return Ok; got {result:?}"
        );
    }

    /// Parity with C-ABI `nmp_signer_broker_init` idempotent second call:
    /// calling it twice before start must still return `Ok(())` (first-writer-wins).
    #[test]
    fn parity_init_signer_broker_idempotent_before_start() {
        let app = crate::NmpApp::new();
        assert!(app.init_signer_broker().is_ok(), "first call must be Ok");
        assert!(app.init_signer_broker().is_ok(), "second call must be Ok (idempotent)");
    }

    /// Parity with C-ABI `nmp_app_cancel_bunker_handshake`:
    /// calling before `init_signer_broker` must not panic (D6 — no broker
    /// handle means the method is a no-op).
    #[test]
    fn parity_cancel_bunker_handshake_before_init_no_panic() {
        let app = crate::NmpApp::new();
        // D6: must not panic even without a prior init_signer_broker.
        app.cancel_bunker_handshake();
    }

    /// Parity with C-ABI `nmp_app_nostrconnect_uri(NULL callback)`:
    /// returns `None` when broker is uninitialised (no relay configured on a
    /// plain new_app — can't produce a URI without a relay).
    #[test]
    fn parity_nostrconnect_uri_before_init_returns_none() {
        let app = crate::NmpApp::new();
        // No broker initialised → None.
        let uri = app.nostrconnect_uri(None);
        assert!(
            uri.is_none(),
            "nostrconnect_uri without init_signer_broker must return None"
        );
    }
}
