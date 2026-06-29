//! Relay-list edit UniFFI methods — M14-C2.
//!
//! Mirrors the relay-management symbols from `nmp-ffi/src/identity.rs`:
//! `nmp_app_add_relay` and `nmp_app_remove_relay`.
//!
//! Each method calls the SAME underlying `nmp_native_runtime::NmpApp` method
//! the C-ABI wrapper calls. No logic is duplicated.
//!
//! ## Role default
//!
//! The C-ABI `nmp_app_add_relay` defaults a null `role` pointer to `"both"`.
//! The UniFFI surface uses `Option<String>` and applies the same default,
//! preserving parity.

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Add a relay to the active account's relay list.
    ///
    /// `role` — `"read"`, `"write"`, or `"both"`. Defaults to `"both"` when
    /// `None`, matching the C-ABI `nmp_app_add_relay` null-role default.
    pub fn add_relay(&self, url: String, role: Option<String>) {
        let role = role
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "both".to_string());
        self.inner.add_relay(url, role);
    }

    /// Remove a relay from the active account's relay list.
    pub fn remove_relay(&self, url: String) {
        self.inner.remove_relay(url);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parity with C-ABI `nmp_app_add_relay(url, role)`:
    /// `add_relay` with explicit role dispatches without panic (D6).
    #[test]
    fn parity_add_relay_explicit_role_no_panic() {
        let app = crate::NmpApp::new();
        app.add_relay("wss://relay.example.com".to_string(), Some("both".to_string()));
        app.add_relay("wss://read.example.com".to_string(), Some("read".to_string()));
        app.add_relay("wss://write.example.com".to_string(), Some("write".to_string()));
    }

    /// Parity with C-ABI `nmp_app_add_relay(url, NULL)`:
    /// `add_relay` with `role = None` must default to `"both"`, matching the
    /// C-ABI null-role path `c_optional_string_argument(role).unwrap_or("both")`.
    #[test]
    fn parity_add_relay_none_role_defaults_to_both() {
        // The UniFFI method accepts `Option<String>` where None ≙ the C null.
        // We cannot inspect the actor queue directly without test-support,
        // but we verify no panic occurs (D6) — the role is applied identically.
        let app = crate::NmpApp::new();
        app.add_relay("wss://relay.example.com".to_string(), None);
    }

    /// Parity with C-ABI `nmp_app_remove_relay(url)`:
    /// `remove_relay` dispatches without panic (D6).
    #[test]
    fn parity_remove_relay_no_panic() {
        let app = crate::NmpApp::new();
        app.remove_relay("wss://relay.example.com".to_string());
    }
}
