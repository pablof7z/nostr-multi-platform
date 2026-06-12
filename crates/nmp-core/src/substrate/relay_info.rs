//! `RelayInfoDoc` — substrate-generic relay-information metadata.
//!
//! This is the carried-through value type for a relay's information document
//! (the NIP-11 `application/nostr+json` payload). It is **substrate-generic
//! transport metadata**, the same class as the existing `RelayStatus` fields:
//! a relay's self-reported name, description, icon, operator identity, software
//! version, and the list of protocol numbers it supports.
//!
//! ## Why this lives in `nmp-core` (D0)
//!
//! D0 keeps *domain nouns* (NIP-29 `group_id`, NIP-94 file metadata, …) out of
//! the substrate. A relay-info roll-up is not a domain noun — it is generic
//! transport telemetry the diagnostics surface already carries (connection
//! state, auth state, byte counters). The kernel needs to *carry* this struct on
//! its per-URL transport row so it flows through `relay_diagnostics_snapshot()`
//! to every consumer app.
//!
//! What `nmp-core` does **not** do: it never performs the HTTP fetch, never
//! parses the JSON, and never names "NIP-11". The fetch + parse logic lives in
//! the Layer-4 `nmp-nip11` protocol crate (ADR-0051), which constructs this
//! struct and hands it back to the actor via
//! [`crate::ActorCommand::SetRelayInfo`]. `nmp-core` imports no HTTP crate.
//!
//! `supported_nips` is `Vec<u32>` (protocol numbers, not nouns). The
//! `limitation_*` booleans surface the cheap, widely-set fields of the NIP-11
//! `limitation` block.

use serde::{Deserialize, Serialize};

/// A relay's self-reported information document, parsed and normalised.
///
/// Every field except `url` is optional: a relay may serve a partial document,
/// or none at all (in which case the kernel simply has no `RelayInfoDoc` for
/// that URL). The shape is the canonical NMP relay-info type — consumer apps
/// read it straight out of the diagnostics projection and never do HTTP, JSON,
/// or learn what NIP-11 is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayInfoDoc {
    /// The relay's `wss://`/`ws://` URL the document was fetched for (stable
    /// identity, set by the fetcher — not a NIP-11 field).
    pub url: String,
    /// Operator-chosen display name.
    pub name: Option<String>,
    /// Human-readable description / "about" text.
    pub description: Option<String>,
    /// Relay icon URL (favicon-style), when advertised.
    pub icon: Option<String>,
    /// Operator administrative public key (hex), when advertised.
    pub pubkey: Option<String>,
    /// Operator contact (email / URL / nostr address), when advertised.
    pub contact: Option<String>,
    /// Relay software identifier (URL or name), when advertised.
    pub software: Option<String>,
    /// Relay software version string, when advertised.
    pub version: Option<String>,
    /// Protocol (NIP) numbers the relay advertises support for. Empty when the
    /// document omits the field.
    pub supported_nips: Vec<u32>,
    /// `limitation.payment_required` — the relay requires payment to write.
    pub limitation_payment_required: Option<bool>,
    /// `limitation.auth_required` — the relay requires NIP-42 AUTH.
    pub limitation_auth_required: Option<bool>,
    /// `limitation.restricted_writes` — writes are restricted to allow-listed
    /// authors.
    pub limitation_restricted_writes: Option<bool>,
}

impl RelayInfoDoc {
    /// Construct an empty document carrying only its `url` identity. Used as
    /// the "fetch resolved but the body was empty / all fields absent" value so
    /// the diagnostics row still records that a fetch completed.
    #[must_use]
    pub fn for_url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Serialise to the JSON string carried by
    /// [`crate::ActorCommand::SetRelayInfo`]. The actor deserialises it back on
    /// the kernel thread. Round-trips through `serde_json`; returns `None` on
    /// the (practically impossible) serialisation error so callers stay
    /// panic-free (D6).
    #[must_use]
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// Parse from the JSON string carried by
    /// [`crate::ActorCommand::SetRelayInfo`]. Returns `None` on malformed input
    /// (D6 — never panics across the actor seam).
    #[must_use]
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let doc = RelayInfoDoc {
            url: "wss://relay.example".to_string(),
            name: Some("Example Relay".to_string()),
            description: Some("A test relay".to_string()),
            icon: Some("https://relay.example/icon.png".to_string()),
            pubkey: Some("deadbeef".to_string()),
            contact: Some("mailto:op@relay.example".to_string()),
            software: Some("strfry".to_string()),
            version: Some("1.0.0".to_string()),
            supported_nips: vec![1, 11, 42],
            limitation_payment_required: Some(false),
            limitation_auth_required: Some(true),
            limitation_restricted_writes: None,
        };
        let json = doc.to_json().expect("serialise");
        let back = RelayInfoDoc::from_json(&json).expect("parse");
        assert_eq!(doc, back);
    }

    #[test]
    fn for_url_carries_only_identity() {
        let doc = RelayInfoDoc::for_url("wss://relay.example");
        assert_eq!(doc.url, "wss://relay.example");
        assert_eq!(doc.name, None);
        assert!(doc.supported_nips.is_empty());
    }

    #[test]
    fn from_json_rejects_garbage_without_panicking() {
        assert_eq!(RelayInfoDoc::from_json("not json"), None);
    }
}
