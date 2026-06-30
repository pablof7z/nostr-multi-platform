//! Account / identity UniFFI methods — M14-C2.
//!
//! Mirrors `nmp-ffi/src/identity.rs` for the sign-in, account-management,
//! and bunker-sign-in symbols. Each method calls the SAME underlying
//! `nmp_native_runtime::NmpApp` method the C-ABI wrapper calls.
//!
//! ## D13 key-handling contract
//!
//! `signin_nsec` and `register_agent_nsec` wrap the caller-supplied string in
//! `zeroize::Zeroizing` the instant it arrives, mirroring the C-ABI
//! wrappers. No raw key bytes are retained past the command dispatch.
//!
use zeroize::Zeroizing;

use crate::identity::RelayConfigEntry;
use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Sign in with a local nsec and optionally make it the active account.
    ///
    /// `make_active = true` (the common path): registers the signer AND makes
    /// it the active account.
    ///
    /// `make_active = false`: registers a visible secondary signer without
    /// activating it.
    ///
    /// D13: the nsec is wrapped in `Zeroizing` immediately; no raw key bytes
    /// are retained past the command dispatch.
    pub fn signin_nsec(&self, secret: String, make_active: bool) {
        let secret = Zeroizing::new(secret);
        self.inner
            .add_signer(nmp_core::SignerSource::LocalNsec(secret), make_active);
    }

    /// Register a persisted app-managed local signer (hidden from account
    /// projections, never becomes the active account).
    ///
    /// D13: the nsec is wrapped in `Zeroizing` immediately.
    pub fn register_agent_nsec(&self, secret: String) {
        let secret = Zeroizing::new(secret);
        self.inner
            .add_signer(nmp_core::SignerSource::AppManagedLocalNsec(secret), false);
    }

    /// Connect a NIP-46 bunker signer.
    ///
    /// `make_active = true`: handshake completes and the resolved pubkey
    /// becomes the active account (the normal bunker sign-in path).
    ///
    /// `make_active = false`: registers the bunker signer WITHOUT activating
    /// it once the handshake completes — for agent/secondary keys.
    pub fn signin_bunker(&self, uri: String, make_active: bool) {
        self.inner
            .add_signer(nmp_core::SignerSource::BunkerUri(uri), make_active);
    }

    /// Create a new account (generate keypair, publish kind:0 + kind:10002).
    ///
    /// `profile` — display-name, picture, about, etc. as key-value pairs.
    /// `relays`  — initial relay list; each entry carries a URL and a role
    ///             string (`"read"`, `"write"`, or `"both"`).
    /// `mls`     — mark the account creation request as MLS-capable. Marmot
    ///             setup remains explicit Rust composition.
    /// `make_active` — make the new account active immediately.
    ///
    /// Auto-follows nobody: generic framework create-account policy (operator
    /// policy lives in the leaf app, not in framework FFI — #1493).
    pub fn create_new_account(
        &self,
        profile: std::collections::HashMap<String, String>,
        relays: Vec<RelayConfigEntry>,
        mls: bool,
        make_active: bool,
    ) {
        let relays: Vec<(String, String)> = relays.into_iter().map(|r| (r.url, r.role)).collect();
        self.inner
            .create_account(profile, relays, Vec::new(), mls, make_active);
    }

    /// Switch the active account to `identity_id` (hex pubkey or account id).
    pub fn switch_active(&self, identity_id: String) {
        self.inner.switch_active(identity_id);
    }

    /// Remove an account from the active session.
    pub fn remove_account(&self, identity_id: String) {
        self.inner.remove_account(identity_id);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parity: `create_new_account` maps typed relays to the underlying
    /// `Vec<(String, String)>` shape correctly (smoke — no panic, correct
    /// relay role round-trip via the typed record).
    #[test]
    fn parity_create_new_account_relay_mapping_no_panic() {
        let app = crate::NmpApp::new();
        let profile = {
            let mut m = std::collections::HashMap::new();
            m.insert("name".to_string(), "TestUser".to_string());
            m
        };
        let relays = vec![
            RelayConfigEntry {
                url: "wss://relay.example.com".to_string(),
                role: "both".to_string(),
            },
            RelayConfigEntry {
                url: "wss://relay2.example.com".to_string(),
                role: "read".to_string(),
            },
        ];
        // D6: must not panic regardless of whether the actor is running.
        app.create_new_account(profile, relays, false, true);
    }

    /// Parity: `switch_active` and `remove_account` dispatch commands without
    /// panicking — D6 fire-and-forget, no actor required.
    #[test]
    fn parity_switch_and_remove_no_panic() {
        let app = crate::NmpApp::new();
        app.switch_active(
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".to_string(),
        );
        app.remove_account(
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".to_string(),
        );
    }
}
