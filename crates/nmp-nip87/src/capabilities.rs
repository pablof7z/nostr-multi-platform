//! Mint-capability (supported NUT) parsing for kind:38172 announcements.
//!
//! A mint advertises which [Cashu NUTs] it supports in two interchangeable
//! places, and different mints in the wild populate one, the other, or both:
//!
//! - a `nuts` tag whose value is a comma/space-separated list of NUT numbers
//!   (e.g. `["nuts", "1,2,3,4,7,8,9,10,11,12"]`); and/or
//! - the event `content`, which MAY carry the mint's NUT-06 `GetInfo` response
//!   JSON, whose `nuts` object keys are the supported NUT numbers (each value
//!   is either a bare `true`/`false` or an object with a `"supported"` flag).
//!
//! [`parse_capabilities`] merges both sources. Downstream nutzap policy in
//! `nmp-wallet` treats a mint that does not advertise the
//! [`NUTZAP_REQUIRED_NUTS`] (NUT-11 P2PK + NUT-12 DLEQ) as unusable and fails
//! closed on it — so capability parsing is a money-safety input, not cosmetic.
//!
//! [Cashu NUTs]: https://github.com/cashubtc/nuts

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The NUTs a mint must advertise to be usable for NIP-61 nutzaps: NUT-11
/// (Pay-to-Pubkey / P2PK locking) and NUT-12 (DLEQ proofs). A mint missing
/// either cannot lock or prove ecash to a recipient key safely, so nutzap
/// policy fails closed on it (see the `nmp-wallet` discovery aggregation).
pub const NUTZAP_REQUIRED_NUTS: [u16; 2] = [11, 12];

/// The set of NUTs (and units, when advertised) a mint supports, parsed from a
/// kind:38172 announcement's tags and/or NUT-06 content.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct MintCapabilities {
    /// Supported NUT numbers (e.g. `{1, 2, 4, 7, 11, 12}`).
    pub nuts: BTreeSet<u16>,
    /// Supported units advertised in the NUT-06 method tables (e.g.
    /// `{"sat", "usd"}`). Empty when the announcement carries no unit info.
    pub units: BTreeSet<String>,
}

impl MintCapabilities {
    /// True when NUT `nut` is advertised as supported.
    #[must_use]
    pub fn supports(&self, nut: u16) -> bool {
        self.nuts.contains(&nut)
    }

    /// True when every NUT in `required` is advertised as supported.
    #[must_use]
    pub fn supports_all(&self, required: &BTreeSet<u16>) -> bool {
        required.is_subset(&self.nuts)
    }

    /// True when the mint advertises the NUTs required for NIP-61 nutzaps
    /// ([`NUTZAP_REQUIRED_NUTS`]: NUT-11 + NUT-12). This is the fail-closed
    /// gate the wallet applies before offering a mint for nutzap use.
    #[must_use]
    pub fn supports_nutzap(&self) -> bool {
        NUTZAP_REQUIRED_NUTS.iter().all(|nut| self.nuts.contains(nut))
    }
}

/// Parse mint capabilities from a kind:38172 event's raw `tags` and `content`.
///
/// Merges the `nuts` tag list and any NUT-06 `nuts` object embedded in the JSON
/// content. Malformed fragments are skipped rather than rejected — a mint that
/// lists valid NUTs alongside a garbage entry still yields the valid set (the
/// fail-closed decision belongs to policy, not this parser).
#[must_use]
pub fn parse_capabilities(tags: &[Vec<String>], content: &str) -> MintCapabilities {
    let mut caps = MintCapabilities::default();

    // 1. `nuts` tag: comma/space-separated NUT numbers.
    for tag in tags {
        if tag.first().map(String::as_str) == Some("nuts") {
            for value in tag.iter().skip(1) {
                for token in value.split(|c: char| c == ',' || c.is_whitespace()) {
                    if let Ok(nut) = token.trim().parse::<u16>() {
                        caps.nuts.insert(nut);
                    }
                }
            }
        }
    }

    // 2. NUT-06 `GetInfo` content JSON (best-effort).
    if let Ok(info) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(nuts) = info.get("nuts").and_then(serde_json::Value::as_object) {
            for (key, value) in nuts {
                let Ok(nut) = key.trim().parse::<u16>() else {
                    continue;
                };
                if nut_value_is_supported(value) {
                    caps.nuts.insert(nut);
                }
                collect_units(value, &mut caps.units);
            }
        }
    }

    caps
}

/// A NUT-06 `nuts` entry counts as supported when it is `true`, or an object
/// whose `"supported"` flag is either absent (present-means-supported) or
/// `true`. An explicit `"supported": false` (some methods advertise themselves
/// as disabled) does not count.
fn nut_value_is_supported(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Object(map) => match map.get("supported") {
            Some(serde_json::Value::Bool(b)) => *b,
            // Method-table NUTs (e.g. NUT-04/05) carry a `methods` array rather
            // than a `supported` bool; their presence means supported.
            _ => true,
        },
        _ => false,
    }
}

/// Collect any `unit` strings from a NUT-06 entry's `methods` array.
fn collect_units(value: &serde_json::Value, units: &mut BTreeSet<String>) {
    if let Some(methods) = value.get("methods").and_then(serde_json::Value::as_array) {
        for method in methods {
            if let Some(unit) = method.get("unit").and_then(serde_json::Value::as_str) {
                units.insert(unit.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nuts_tag_comma_separated() {
        let tags = vec![vec![
            "nuts".to_string(),
            "1,2,3,4,7,8,9,10,11,12".to_string(),
        ]];
        let caps = parse_capabilities(&tags, "");
        assert!(caps.supports(1));
        assert!(caps.supports_nutzap());
        assert_eq!(caps.nuts.len(), 10);
    }

    #[test]
    fn parses_nut06_content_object() {
        let content = r#"{
            "nuts": {
                "4": {"methods": [{"method": "bolt11", "unit": "sat"}], "disabled": false},
                "5": {"methods": [{"method": "bolt11", "unit": "usd"}]},
                "7": {"supported": true},
                "11": {"supported": true},
                "12": {"supported": true}
            }
        }"#;
        let caps = parse_capabilities(&[], content);
        assert!(caps.supports_nutzap());
        assert!(caps.supports(4) && caps.supports(5) && caps.supports(7));
        assert_eq!(
            caps.units,
            BTreeSet::from(["sat".to_string(), "usd".to_string()])
        );
    }

    #[test]
    fn explicit_unsupported_flag_excludes_the_nut() {
        let content = r#"{"nuts": {"11": {"supported": false}, "12": {"supported": true}}}"#;
        let caps = parse_capabilities(&[], content);
        assert!(!caps.supports(11));
        assert!(caps.supports(12));
        assert!(!caps.supports_nutzap(), "missing NUT-11 must fail nutzap gate");
    }

    #[test]
    fn merges_tag_and_content_sources() {
        let tags = vec![vec!["nuts".to_string(), "11".to_string()]];
        let content = r#"{"nuts": {"12": true}}"#;
        let caps = parse_capabilities(&tags, content);
        assert!(caps.supports_nutzap());
    }

    #[test]
    fn garbage_fragments_are_skipped_not_fatal() {
        let tags = vec![vec![
            "nuts".to_string(),
            "11, not-a-number, 12".to_string(),
        ]];
        let caps = parse_capabilities(&tags, "not json at all");
        assert_eq!(caps.nuts, BTreeSet::from([11, 12]));
    }

    #[test]
    fn empty_announcement_supports_nothing() {
        let caps = parse_capabilities(&[], "");
        assert!(caps.nuts.is_empty());
        assert!(!caps.supports_nutzap());
    }
}
