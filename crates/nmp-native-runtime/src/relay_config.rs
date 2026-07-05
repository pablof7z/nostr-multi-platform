//! Relay configuration helpers for the native runtime.
//!
//! The sidecar file (`{storage_dir}/.nmp-relay-config.json`) stores the full
//! (url, role) list. Written on first start from the builder-declared
//! defaults, and re-written on every genuine `configured_relays` change (the
//! `app_ctor::new_app`-installed observer calls [`persist_configured_relays`]
//! — see #3059: previously `save()` was only ever invoked once, at first
//! start, so any relay added afterward via `add_relay`/`remove_relay`
//! dispatch (or an inbound kind:10002 relay-list sync) was never written
//! back. A cold relaunch then reloaded the stale first-run set, and
//! downstream `nmp-nip17`'s `DmRuntimeController` faithfully republished
//! kind:10050 from that stale, narrower set — silently dropping a relay
//! (e.g. the user's DM-inbox relay) the account genuinely still had. Read on
//! every subsequent start.
//!
//! This is the *app-template* (composition-root) home for the relay default
//! set — `nmp-core` no longer carries any hardcoded relay fallback. The app
//! declares its relays through `NmpAppBuilder`, those defaults are persisted
//! here on first run, and subsequent runs reload the user's edited list.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use nmp_core::__ffi_internal::has_role;

use crate::{NmpApp, NmpConfigStatus};

pub(crate) const RELAY_CONFIG_FILENAME: &str = ".nmp-relay-config.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RelayEntry {
    url: String,
    role: String,
}

/// Load relay config from the sidecar file. Returns `None` if the file doesn't
/// exist, cannot be parsed, or is empty.
pub(crate) fn load(storage_dir: &Path) -> Option<Vec<(String, String)>> {
    let path = storage_dir.join(RELAY_CONFIG_FILENAME);
    let content = std::fs::read_to_string(&path).ok()?;
    let entries: Vec<RelayEntry> = serde_json::from_str(&content).ok()?;
    if entries.is_empty() {
        return None;
    }
    Some(entries.into_iter().map(|e| (e.url, e.role)).collect())
}

/// Write relay config to the sidecar file. Silently no-ops on I/O errors.
pub(crate) fn save(storage_dir: &Path, relays: &[(String, String)]) {
    let entries: Vec<RelayEntry> = relays
        .iter()
        .map(|(url, role)| RelayEntry {
            url: url.clone(),
            role: role.clone(),
        })
        .collect();
    if let Ok(json) = serde_json::to_string_pretty(&entries) {
        let path = storage_dir.join(RELAY_CONFIG_FILENAME);
        let _ = std::fs::write(&path, json);
    }
}

/// Persist the current configured-relay rows to the on-disk sidecar so a
/// cold relaunch reloads the FULL set that was actually active, not the
/// stale first-run defaults (#3059 — see the module doc for the full
/// root-cause story).
///
/// Called from the `configured_relays`-change observer `app_ctor::new_app`
/// installs on every `NmpApp`; fires on every genuine change (add/remove
/// relay dispatch, an inbound kind:10002 relay-list sync, ...), so the
/// sidecar always mirrors the live in-memory set. A no-op when
/// `storage_dir` is `None` — in-memory apps (no `.storage_path(...)`) carry
/// no sidecar and must not gain one here.
pub(crate) fn persist_configured_relays(storage_dir: Option<&Path>, relays: &[(String, String)]) {
    if let Some(dir) = storage_dir {
        save(dir, relays);
    }
}

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
            if let Some(url) = configured_nostrconnect_relay_url(
                guard.as_slice().iter().map(|row| (row.url(), row.role())),
            ) {
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
    /// handshake.
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

fn configured_nostrconnect_relay_url<'a, I>(rows: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    nmp_router::nostrconnect_bootstrap_relay_url(rows, |role| has_role(role, "write"))
}

#[cfg(test)]
#[path = "relay_config_tests.rs"]
mod tests;
