//! Relay configuration helpers for the native runtime.
//!
//! The sidecar file (`{storage_dir}/.nmp-relay-config.json`) stores the full
//! (url, role) list. Written on first start from the builder-declared defaults;
//! updated by `add_relay`/`remove_relay` dispatch (future work: hook the
//! dispatch callback). Read on every subsequent start.
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
mod tests {
    use super::*;

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!(
            "nmp-relay-config-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = unique_temp_dir("roundtrip");
        let relays = vec![
            (
                "wss://primary-relay.example".to_string(),
                "both,indexer".to_string(),
            ),
            (
                "wss://indexer-relay.example".to_string(),
                "indexer".to_string(),
            ),
        ];
        save(&dir, &relays);
        let loaded = load(&dir).expect("sidecar loads after save");
        assert_eq!(loaded, relays);
    }

    #[test]
    fn load_missing_file_returns_none() {
        let dir = unique_temp_dir("missing");
        // No save() call — the sidecar does not exist.
        assert!(load(&dir).is_none(), "missing sidecar must yield None");
    }

    #[test]
    fn load_empty_array_returns_none() {
        let dir = unique_temp_dir("empty");
        // Persist an explicitly empty list, then confirm load treats it as
        // "nothing configured" (None) so the builder falls back to defaults.
        save(&dir, &[]);
        assert!(
            load(&dir).is_none(),
            "an empty sidecar array must be treated as None, not Some(vec![])"
        );
    }

    #[test]
    fn load_malformed_json_returns_none() {
        let dir = unique_temp_dir("malformed");
        let path = dir.join(RELAY_CONFIG_FILENAME);
        std::fs::write(&path, b"{ this is not valid json").expect("write malformed");
        assert!(load(&dir).is_none(), "unparseable sidecar must yield None");
    }

    #[test]
    fn nostrconnect_configured_selection_uses_router_policy_and_core_roles() {
        let rows = [
            ("read-relay", "read"),
            ("write-relay", "write"),
            ("both-relay", "both"),
        ];

        assert_eq!(
            configured_nostrconnect_relay_url(rows),
            Some("write-relay".to_string())
        );
    }

    #[test]
    fn nostrconnect_configured_selection_accepts_composite_role() {
        let rows = [
            ("indexer-relay", "indexer"),
            ("composite-relay", "both,indexer"),
        ];

        assert_eq!(
            configured_nostrconnect_relay_url(rows),
            Some("composite-relay".to_string())
        );
    }

    #[test]
    fn nostrconnect_relay_url_falls_back_to_registered_bootstrap() {
        let app = crate::new_app();
        assert_eq!(
            app.set_nostrconnect_bootstrap_relay("bootstrap-relay".to_string()),
            NmpConfigStatus::Ok
        );

        assert_eq!(
            app.nostrconnect_relay_url(),
            Some("bootstrap-relay".to_string())
        );
    }
}
