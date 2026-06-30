//! NIP-11 `application/nostr+json` document parsing.
//!
//! The wire shape is a single JSON object with all fields optional (NIP-11):
//! `name`, `description`, `icon`, `pubkey`, `contact`, `software`, `version`,
//! `supported_nips` (array of numbers), a nested `limitation` object, and
//! arbitrary NIP-specific capability objects (e.g. `nip29`). We parse
//! tolerantly: unknown fields are ignored, missing fields become `None`, and
//! a body that is not a JSON object is an error (the caller records the relay
//! as having no document).
//!
//! Output is the substrate-generic [`RelayInfoDoc`] so the parsed shape flows
//! straight into the kernel's diagnostics surface without any nmp-nip11 type
//! crossing the actor seam.

use std::collections::BTreeMap;

use nmp_core::substrate::RelayInfoDoc;
use serde::Deserialize;

/// The key a relay's NIP-11 `nip29.subgroups` capability is filed under in
/// [`RelayInfoDoc::feature_flags`] (nips PR #2319). Authored here — in the
/// Layer-4 NIP-11 crate — not in `nmp-core`, so the substrate stays free of
/// NIP nouns (D0).
pub const NIP29_SUBGROUPS_FLAG: &str = "nip29.subgroups";

/// The raw NIP-11 wire object. Every field optional; `supported_nips` is parsed
/// leniently as floats-then-truncated so a relay emitting `1.0` still maps to
/// `1` (some relays do). Unknown keys are dropped by serde's default behaviour.
#[derive(Debug, Default, Deserialize)]
struct WireDoc {
    name: Option<String>,
    description: Option<String>,
    icon: Option<String>,
    pubkey: Option<String>,
    contact: Option<String>,
    software: Option<String>,
    version: Option<String>,
    #[serde(default)]
    supported_nips: Vec<serde_json::Value>,
    #[serde(default)]
    limitation: WireLimitation,
    /// NIP-29 subgroups capability (nips PR #2319). Unknown keys inside the
    /// object are dropped by serde's default behaviour.
    #[serde(default)]
    nip29: WireNip29,
}

#[derive(Debug, Default, Deserialize)]
struct WireLimitation {
    payment_required: Option<bool>,
    auth_required: Option<bool>,
    restricted_writes: Option<bool>,
}

/// The NIP-11 `nip29` capability object (nips PR #2319). Only the
/// `subgroups` boolean is read; unknown keys are ignored.
#[derive(Debug, Default, Deserialize)]
struct WireNip29 {
    #[serde(default)]
    subgroups: Option<bool>,
}

/// Parse a NIP-11 document body into a [`RelayInfoDoc`] tagged with `relay_url`.
///
/// `relay_url` is the original `wss://`/`ws://` URL (the document's stable
/// identity); it is NOT taken from the body. Returns an error string when the
/// body is not a JSON object — callers treat that as "relay serves no document".
///
/// Empty / absent string fields are normalised to `None`; `supported_nips`
/// entries that are not non-negative integers in `u32` range are dropped.
pub fn parse_relay_info(relay_url: &str, body: &[u8]) -> Result<RelayInfoDoc, String> {
    let wire: WireDoc =
        serde_json::from_slice(body).map_err(|e| format!("parse NIP-11 document: {e}"))?;

    let supported_nips = wire
        .supported_nips
        .iter()
        .filter_map(value_to_nip)
        .collect();

    // NIP-specific capability booleans → the substrate-generic `feature_flags`
    // carrier. The key string ("nip29.subgroups") is authored here, not in
    // nmp-core, so D0 holds. Only `Some(true)` is recorded; `None`/`false`
    // mean the relay does not advertise the capability.
    let mut feature_flags = BTreeMap::new();
    if wire.nip29.subgroups == Some(true) {
        feature_flags.insert(NIP29_SUBGROUPS_FLAG.to_string(), true);
    }

    Ok(RelayInfoDoc {
        url: relay_url.to_string(),
        name: non_empty(wire.name),
        description: non_empty(wire.description),
        icon: non_empty(wire.icon),
        pubkey: non_empty(wire.pubkey),
        contact: non_empty(wire.contact),
        software: non_empty(wire.software),
        version: non_empty(wire.version),
        supported_nips,
        limitation_payment_required: wire.limitation.payment_required,
        limitation_auth_required: wire.limitation.auth_required,
        limitation_restricted_writes: wire.limitation.restricted_writes,
        feature_flags,
    })
}

/// Trim and drop empty strings to `None` so the diagnostics surface never shows
/// a blank name/description.
fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Coerce one `supported_nips` JSON entry to a `u32`. Accepts integers and
/// whole-valued floats; rejects negatives, fractionals, out-of-range, and
/// non-numeric entries (NIP-11 lists numbers, but some relays emit `"1"` —
/// accept a numeric string too).
fn value_to_nip(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f <= f64::from(u32::MAX) {
                Some(f as u32)
            } else {
                None
            }
        }
        serde_json::Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_document() {
        let body = br#"{
            "name": "Example Relay",
            "description": "A test relay",
            "icon": "https://relay.example/icon.png",
            "pubkey": "deadbeef",
            "contact": "mailto:op@relay.example",
            "software": "git+https://github.com/example/relay",
            "version": "1.2.3",
            "supported_nips": [1, 11, 42, 50],
            "limitation": {
                "payment_required": true,
                "auth_required": false,
                "restricted_writes": true
            }
        }"#;
        let doc = parse_relay_info("wss://relay.example", body).expect("parse");
        assert_eq!(doc.url, "wss://relay.example");
        assert_eq!(doc.name.as_deref(), Some("Example Relay"));
        assert_eq!(doc.description.as_deref(), Some("A test relay"));
        assert_eq!(doc.icon.as_deref(), Some("https://relay.example/icon.png"));
        assert_eq!(doc.pubkey.as_deref(), Some("deadbeef"));
        assert_eq!(doc.contact.as_deref(), Some("mailto:op@relay.example"));
        assert_eq!(doc.version.as_deref(), Some("1.2.3"));
        assert_eq!(doc.supported_nips, vec![1, 11, 42, 50]);
        assert_eq!(doc.limitation_payment_required, Some(true));
        assert_eq!(doc.limitation_auth_required, Some(false));
        assert_eq!(doc.limitation_restricted_writes, Some(true));
    }

    #[test]
    fn parses_an_empty_object_into_url_only() {
        let doc = parse_relay_info("wss://relay.example", b"{}").expect("parse");
        assert_eq!(doc.url, "wss://relay.example");
        assert_eq!(doc.name, None);
        assert!(doc.supported_nips.is_empty());
        assert_eq!(doc.limitation_payment_required, None);
    }

    #[test]
    fn ignores_unknown_fields() {
        let body = br#"{"name": "R", "fees": {"admission": []}, "future_field": 1}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert_eq!(doc.name.as_deref(), Some("R"));
    }

    #[test]
    fn partial_document_leaves_absent_fields_none() {
        let body = br#"{"name": "Only Name", "supported_nips": [1]}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert_eq!(doc.name.as_deref(), Some("Only Name"));
        assert_eq!(doc.description, None);
        assert_eq!(doc.supported_nips, vec![1]);
    }

    #[test]
    fn blank_strings_become_none() {
        let body = br#"{"name": "   ", "description": ""}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert_eq!(doc.name, None);
        assert_eq!(doc.description, None);
    }

    #[test]
    fn drops_malformed_supported_nips_entries() {
        let body = br#"{"supported_nips": [1, -3, 2.5, "11", "x", 70000000000]}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        // 1 ok, -3 dropped, 2.5 dropped, "11" coerced, "x" dropped, huge dropped
        assert_eq!(doc.supported_nips, vec![1, 11]);
    }

    #[test]
    fn non_object_body_is_an_error() {
        assert!(parse_relay_info("wss://r", b"not json").is_err());
        assert!(parse_relay_info("wss://r", b"[1,2,3]").is_err());
    }

    #[test]
    fn nip29_subgroups_true_is_recorded_as_a_feature_flag() {
        // nips PR #2319: a relay advertises subgroup support via
        // `{"nip29": {"subgroups": true}}` in its NIP-11 doc.
        let body = br#"{"supported_nips": [29], "nip29": {"subgroups": true}}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert_eq!(
            doc.feature_flags.get("nip29.subgroups"),
            Some(&true),
            "nip29.subgroups=true must be filed under feature_flags"
        );
    }

    #[test]
    fn nip29_subgroups_false_or_absent_is_not_recorded() {
        // Explicitly false: not a capability the relay advertises.
        let body = br#"{"nip29": {"subgroups": false}}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert!(
            !doc.feature_flags.contains_key("nip29.subgroups"),
            "subgroups=false must not be recorded as an advertised capability"
        );
        // Absent entirely.
        let doc = parse_relay_info("wss://r", b"{}").expect("parse");
        assert!(doc.feature_flags.is_empty());
    }

    #[test]
    fn unknown_keys_in_the_nip29_object_are_ignored() {
        let body = br#"{"nip29": {"subgroups": true, "future_field": 7}}"#;
        let doc = parse_relay_info("wss://r", body).expect("parse");
        assert_eq!(doc.feature_flags.get("nip29.subgroups"), Some(&true));
        // Only the known boolean is filed; the unknown key never reaches
        // `feature_flags`.
        assert_eq!(doc.feature_flags.len(), 1);
    }
}
