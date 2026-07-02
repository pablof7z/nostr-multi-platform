//! `GroupId { host_relay_url, local_id }` — the typed group identity.
//!
//! NIP-29 identifies a group by the **pair** `(host_relay_url, local_id)`.
//! Two relays running with the same `local_id` are two different groups.
//! Highlighter's existing core (`/Users/pablofernandez/Work/hl/app/core/src/`)
//! dodges this by hard-coding `HIGHLIGHTER_RELAY`; NMP cannot.
//!
//! This module is the only place in the crate that knows how to round-trip
//! a `GroupId` to and from the NIP-29 spec URI shape `<host>'<local-id>`
//! (e.g. `groups.nostr.com'abcdef`). Every other module uses the typed
//! `GroupId` and never inspects the wire string.
//!
//! Design: `docs/design/nip29-crate.md` §5.

use serde::{Deserialize, Serialize};

/// Re-export of the single workspace `RelayUrl` authority, owned at Layer 0 by
/// [`nmp_relay_url::RelayUrl`].
pub use nmp_relay_url::RelayUrl;

/// NIP-29 group identity: the host relay URL plus the in-relay local id.
///
/// `host_relay_url` is a `wss://` URL; canonicalisation rules (trailing slash,
/// case-insensitive scheme/host, default port) follow the NIP-65
/// url-canonicalisation pre-rules.
///
/// `local_id` matches the NIP-29 charset `[a-z0-9-_]+`.
#[derive(Clone, Debug, Default, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct GroupId {
    pub host_relay_url: RelayUrl,
    pub local_id: String,
}

impl GroupId {
    /// Construct from owned strings.
    #[must_use]
    pub fn new(host_relay_url: impl Into<RelayUrl>, local_id: impl Into<String>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            local_id: local_id.into(),
        }
    }

    /// Check that both `host_relay_url` and `local_id` are non-empty.
    /// All group actions that route to a specific relay must call this in
    /// `start()` to prevent silent routing to relay `""`.
    ///
    /// # Errors
    ///
    /// Returns a string describing the missing field if either is empty.
    pub fn require_routable(&self) -> Result<(), String> {
        if self.host_relay_url.is_empty() {
            return Err("group.host_relay_url must not be empty".into());
        }
        if self.local_id.is_empty() {
            return Err("group.local_id must not be empty".into());
        }
        Ok(())
    }

    /// Encode as the NIP-29 URI shape `<host>'<local-id>`.
    ///
    /// `<host>` is the *bare host* part of the relay URL (scheme + `://`
    /// stripped, trailing slash stripped). Per the NIP-29 spec, the encoded
    /// form is intended to be human-shareable; callers wanting the full
    /// `wss://` form should use `host_relay_url` directly.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let host = strip_ws_scheme(&self.host_relay_url);
        format!("{host}'{}", self.local_id)
    }

    /// Parse from the NIP-29 URI shape `<host>'<local-id>`.
    ///
    /// Returns `None` if the string does not contain exactly one `'`, has an
    /// empty host or local id, or the `local_id` contains characters outside
    /// the NIP-29 charset `[a-z0-9-_]+`. The host is rewrapped as
    /// `wss://<host>` since the URI form omits the scheme.
    #[must_use]
    pub fn from_uri(s: &str) -> Option<Self> {
        let (host, local) = s.split_once('\'')?;
        if host.is_empty() || local.is_empty() {
            return None;
        }
        if !local.chars().all(is_nip29_local_id_char) {
            return None;
        }
        Some(Self::new(format!("wss://{host}"), local))
    }
}

/// NIP-01 REQ filter JSON for relay-signed group metadata
/// (kind:39000 / 39001 / 39002), with no `d` filter so every group a relay
/// hosts surfaces.
///
/// Shared by the discovery view (Global scope, host-relay-pinned) and the
/// joined-groups view (ActiveAccount scope). NmpApp-free and noun-free of the
/// FFI host: the composition root (`nmp-ffi`) passes the result to
/// `NmpApp::open_observed_interest_pinned`, attaching the client-side relay
/// pin as a separate argument (the pin is never serialized onto the wire).
///
/// `39003` (roles) is intentionally excluded — the read projections fold only
/// 39000/39001/39002.
#[must_use]
pub fn group_metadata_filter_json() -> String {
    serde_json::json!({
        "kinds": [
            crate::kinds::KIND_GROUP_METADATA,
            crate::kinds::KIND_GROUP_ADMINS,
            crate::kinds::KIND_GROUP_MEMBERS,
        ],
    })
    .to_string()
}

/// Relay filter for a single group's **roster**: the relay-signed 39001
/// (admins) / 39002 (members) / 39003 (roles) snapshots scoped to one group's
/// `d` identifier.
///
/// Unlike [`group_metadata_filter_json`] (which subscribes to the catalog-wide
/// metadata kinds with no `d` constraint), this filter is `["#d", [group_id]]`
/// scoped so the subscription only delivers the one group's roster events, and
/// it INCLUDES 39003 (roles) — the roster projection folds the role catalog,
/// whereas the count-only discovery/joined views deliberately do not.
///
/// NmpApp-free: the composition root passes the result to the relay-pinned
/// observed-projection door, attaching the host relay pin separately.
#[must_use]
pub fn group_roster_filter_json(group_id: &str) -> String {
    serde_json::json!({
        "kinds": [
            crate::kinds::KIND_GROUP_ADMINS,
            crate::kinds::KIND_GROUP_MEMBERS,
            crate::kinds::KIND_GROUP_ROLES,
        ],
        "#d": [group_id],
    })
    .to_string()
}

fn strip_ws_scheme(url: &str) -> &str {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url)
        .trim_end_matches('/')
}

fn is_nip29_local_id_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_uri() {
        let g = GroupId::new("wss://groups.nostr.com", "abc-123");
        let uri = g.to_uri();
        assert_eq!(uri, "groups.nostr.com'abc-123");
        assert_eq!(GroupId::from_uri(&uri), Some(g));
    }

    #[test]
    fn parse_rejects_uppercase_local() {
        // NIP-29 local id charset is [a-z0-9-_]+; uppercase is invalid.
        assert!(GroupId::from_uri("groups.example.com'ABC").is_none());
    }

    #[test]
    fn parse_rejects_no_separator() {
        assert!(GroupId::from_uri("no-tick-here").is_none());
    }

    #[test]
    fn parse_rejects_empty_local() {
        assert!(GroupId::from_uri("groups.example.com'").is_none());
    }

    #[test]
    fn require_routable_passes_when_both_fields_present() {
        let g = GroupId::new("wss://h", "room");
        assert!(g.require_routable().is_ok());
    }

    #[test]
    fn require_routable_rejects_empty_host_relay_url() {
        let g = GroupId::new("", "room");
        assert!(g.require_routable().is_err());
    }

    #[test]
    fn require_routable_rejects_empty_local_id() {
        let g = GroupId::new("wss://h", "");
        assert!(g.require_routable().is_err());
    }

    #[test]
    fn group_metadata_filter_json_targets_three_metadata_kinds_no_d() {
        let v: serde_json::Value = serde_json::from_str(&group_metadata_filter_json()).unwrap();
        assert_eq!(v["kinds"], serde_json::json!([39000, 39001, 39002]));
        // Discovery is per-relay, not per-group — no `d` (or `#d`) constraint.
        assert!(v.get("#d").is_none() && v.get("d").is_none());
        assert!(
            nmp_planner::InterestShape::from_filter_json(&group_metadata_filter_json()).is_some()
        );
    }

    #[test]
    fn strip_scheme_handles_ws_and_trailing_slash() {
        assert_eq!(strip_ws_scheme("wss://x/"), "x");
        assert_eq!(strip_ws_scheme("ws://y"), "y");
        assert_eq!(strip_ws_scheme("plain"), "plain");
    }
}
