//! Rust-side relay configuration helpers for app composition crates.

use std::sync::Arc;

use nmp_core::__ffi_internal::{has_role, nostrconnect_relay_url};

use crate::NmpApp;

impl NmpApp {
    /// Clone of the live relay-edit row slot.
    #[must_use]
    pub fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        Arc::clone(&self.configured_relays)
    }

    /// Store the initial relay configuration passed into actor start.
    pub fn set_initial_relays_for_start(&self, relays: Vec<(String, String)>) {
        if let Ok(mut guard) = self.initial_relays_for_start.lock() {
            *guard = relays;
        }
    }

    /// Return the user's current write-relay URLs.
    #[must_use]
    pub fn write_relay_urls(&self) -> Vec<String> {
        let Ok(guard) = self.configured_relays.lock() else {
            return Vec::new();
        };
        guard
            .as_slice()
            .iter()
            .filter(|r| has_role(r.role(), "write"))
            .map(|r| r.url().to_string())
            .collect()
    }

    /// Choose the relay for a client-initiated NIP-46 `nostrconnect://` handshake.
    #[must_use]
    pub fn nostrconnect_relay_url(&self) -> Option<String> {
        if let Ok(guard) = self.configured_relays.lock() {
            if let Some(url) =
                nostrconnect_relay_url(guard.as_slice().iter().map(|row| (row.url(), row.role())))
            {
                return Some(url);
            }
        }
        self.nostrconnect_bootstrap_relay
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub(crate) fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        if let Ok(mut guard) = self.nostrconnect_bootstrap_relay.lock() {
            *guard = Some(url);
        }
    }
}
