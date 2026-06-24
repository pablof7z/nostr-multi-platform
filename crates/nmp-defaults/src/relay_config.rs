//! JSON sidecar persistence for the app's configured relay list.
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
}
