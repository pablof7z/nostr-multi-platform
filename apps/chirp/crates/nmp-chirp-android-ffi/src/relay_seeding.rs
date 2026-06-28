//! Relay bootstrap seeding for the Android JNI shim.
//!
//! Seeding policy lives here, not in Kotlin (D7 / thin-shell principle):
//!
//! * `seed_default_relays` — adds the Chirp reference set
//!   (`nmp-chirp-config::chirp_default_relay_bootstrap`).
//! * `seed_relays_from_json` — parses a `[["url","role"],…]` JSON array
//!   and adds each entry; used by the `NMP_TEST_RELAYS` override path.
//! * `default_relays_json_array` — serialises the default set into the
//!   `[["url","role"],…]` shape expected by `nmp_app_create_new_account`.

use std::ffi::CString;

use nmp_ffi::{nmp_app_add_relay, NmpApp};

/// Seed the Chirp reference relay set.
///
/// Called from `nativeSeedRelays` when no override is present (production
/// path) and as a D6 fallback when the override JSON fails to parse.
pub(crate) fn seed_default_relays(app: *mut NmpApp) {
    for entry in nmp_chirp_config::chirp_default_relay_bootstrap() {
        let Ok(url) = CString::new(entry.url) else {
            continue;
        };
        let Ok(role) = CString::new(entry.role) else {
            continue;
        };
        nmp_app_add_relay(app, url.as_ptr(), role.as_ptr());
    }
}

/// Seed relays from a JSON array of `["url", "role"]` pairs.
///
/// Returns `true` when at least one entry was seeded successfully, `false`
/// when the JSON is malformed or empty — the caller should fall back to
/// `seed_default_relays` in that case.
///
/// Parsing and policy live here, not in Kotlin (D7 / thin-shell principle).
pub(crate) fn seed_relays_from_json(app: *mut NmpApp, json: &str) -> bool {
    let parsed: Vec<[String; 2]> = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if parsed.is_empty() {
        return false;
    }
    let mut seeded = false;
    for entry in &parsed {
        let Ok(url) = CString::new(entry[0].as_str()) else {
            continue;
        };
        let Ok(role) = CString::new(entry[1].as_str()) else {
            continue;
        };
        nmp_app_add_relay(app, url.as_ptr(), role.as_ptr());
        seeded = true;
    }
    seeded
}

/// Serialise the Chirp reference relay set as `[["url","role"],…]` JSON —
/// the shape expected by `nmp_app_create_new_account`.
pub(crate) fn default_relays_json_array() -> String {
    let relays: Vec<[&str; 2]> = nmp_chirp_config::chirp_default_relay_bootstrap()
        .iter()
        .map(|e| [e.url, e.role])
        .collect();
    serde_json::to_string(&relays).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::seed_relays_from_json;
    use std::ptr;

    fn null_app() -> *mut nmp_ffi::NmpApp {
        ptr::null_mut()
    }

    #[test]
    fn relay_override_empty_array_returns_false() {
        // An empty array must signal "no relays seeded" so the caller
        // falls back to the defaults.
        assert!(!seed_relays_from_json(null_app(), "[]"));
    }

    #[test]
    fn relay_override_malformed_json_returns_false() {
        assert!(!seed_relays_from_json(null_app(), "not json"));
        assert!(!seed_relays_from_json(null_app(), "{}"));
        assert!(!seed_relays_from_json(null_app(), "[[]]")); // wrong inner shape
    }

    #[test]
    fn relay_override_valid_single_entry_returns_true() {
        // With a null NmpApp, `nmp_app_add_relay` is called with null and
        // returns immediately (D6). The function should still return `true`
        // because the JSON was valid and a CString pair was produced.
        //
        // Note: we cannot assert the relay was *added* without a live kernel,
        // but the return value contract (true = valid JSON, ≥1 entry) is
        // testable here.
        let json = r#"[["ws://127.0.0.1:10547","both"]]"#;
        assert!(seed_relays_from_json(null_app(), json));
    }

    #[test]
    fn relay_override_multiple_entries_returns_true() {
        let json = r#"[["wss://relay.example.com","both"],["wss://indexer.example.com","indexer"]]"#;
        assert!(seed_relays_from_json(null_app(), json));
    }

    #[test]
    fn default_relays_json_array_is_non_empty() {
        let json = super::default_relays_json_array();
        let parsed: Vec<[String; 2]> = serde_json::from_str(&json).expect("valid JSON");
        assert!(!parsed.is_empty(), "Chirp must have at least one reference relay");
        for entry in &parsed {
            assert!(!entry[0].is_empty(), "relay URL must not be empty");
            assert!(!entry[1].is_empty(), "relay role must not be empty");
        }
    }
}
