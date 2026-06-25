//! Rust-side relay configuration helpers for app composition crates.

use std::sync::Arc;

use nmp_core::__ffi_internal::{has_role, nostrconnect_relay_url};

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Clone of the live relay-edit row slot.
    #[must_use]
    pub fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        Arc::clone(&self.configured_relays)
    }

    /// Store the initial relay configuration passed into actor start.
    pub fn set_initial_relays_for_start(&self, relays: Vec<(String, String)>) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "initial_relays_for_start",
            "initial_relays_for_start",
            "initial_relays_for_start",
        ) {
            return status;
        }
        if let Ok(mut guard) = self.composition.initial_relays_for_start.lock() {
            self.record_slot_decision(
                "initial_relays_for_start",
                "initial_relays_for_start",
                !guard.is_empty(),
            );
            *guard = relays;
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
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
        self.composition
            .nostrconnect_bootstrap_relay
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub(crate) fn set_nostrconnect_bootstrap_relay(&self, url: String) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "nostrconnect_bootstrap_relay",
            "nostrconnect_bootstrap_relay",
            "nostrconnect_bootstrap_relay",
        ) {
            return status;
        }
        if let Ok(mut guard) = self.composition.nostrconnect_bootstrap_relay.lock() {
            // ADR-0049 Part 2 — record the last-writer-wins decision for this
            // slot (Installed / ReplacedPrevious / DroppedLateWiring).
            self.record_slot_decision(
                "nostrconnect_bootstrap_relay",
                "nostrconnect_bootstrap_relay",
                guard.is_some(),
            );
            *guard = Some(url);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Read the host-supplied NIP-46 perm request for a `nostrconnect://`
    /// handshake. `None` means NMP supplies no perms (#1493): the broker omits
    /// the `&perms=` URI parameter entirely. The returned string is the plain
    /// (NOT percent-encoded) comma-joined NIP-46 perm list.
    #[must_use]
    pub fn nostrconnect_perms(&self) -> Option<String> {
        self.composition
            .nostrconnect_perms
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub(crate) fn set_nostrconnect_perms(&self, perms: String) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "nostrconnect_perms",
            "nostrconnect_perms",
            "nostrconnect_perms",
        ) {
            return status;
        }
        if let Ok(mut guard) = self.composition.nostrconnect_perms.lock() {
            // ADR-0049 Part 2 — record the last-writer-wins decision for this
            // slot (Installed / ReplacedPrevious / DroppedLateWiring).
            self.record_slot_decision("nostrconnect_perms", "nostrconnect_perms", guard.is_some());
            *guard = Some(perms);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    pub(crate) fn set_relay_user_agent(&self, user_agent: String) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("relay_user_agent", "relay_user_agent", "relay_user_agent")
        {
            return status;
        }
        if let Ok(mut guard) = self.composition.user_agent.lock() {
            self.record_slot_decision("relay_user_agent", "relay_user_agent", guard.is_some());
            *guard = Some(user_agent);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    pub(crate) fn set_outbound_public_tags(&self, tags: Vec<Vec<String>>) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "outbound_public_tags",
            "outbound_public_tags",
            "outbound_public_tags",
        ) {
            return status;
        }
        if let Ok(mut guard) = self.composition.outbound_public_tags.lock() {
            self.record_slot_decision(
                "outbound_public_tags",
                "outbound_public_tags",
                guard.is_some(),
            );
            *guard = Some(tags);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }
}
