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

/// Re-export of the kernel's `RelayUrl` alias to keep the crate's surface
/// self-describing while avoiding a circular dep on `nmp-core` types.
pub type RelayUrl = String;

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
    pub fn new(host_relay_url: impl Into<RelayUrl>, local_id: impl Into<String>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            local_id: local_id.into(),
        }
    }

    /// Encode as the NIP-29 URI shape `<host>'<local-id>`.
    ///
    /// `<host>` is the *bare host* part of the relay URL (scheme + `://`
    /// stripped, trailing slash stripped). Per the NIP-29 spec, the encoded
    /// form is intended to be human-shareable; callers wanting the full
    /// `wss://` form should use `host_relay_url` directly.
    pub fn to_uri(&self) -> String {
        let host = strip_ws_scheme(&self.host_relay_url);
        format!("{host}'{}", self.local_id)
    }

    /// Parse from the NIP-29 URI shape `<host>'<local-id>`.
    ///
    /// Returns `None` if the string does not contain exactly one `'`, has an
    /// empty host or local id, or the local_id contains characters outside
    /// the NIP-29 charset `[a-z0-9-_]+`. The host is rewrapped as
    /// `wss://<host>` since the URI form omits the scheme.
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
    fn strip_scheme_handles_ws_and_trailing_slash() {
        assert_eq!(strip_ws_scheme("wss://x/"), "x");
        assert_eq!(strip_ws_scheme("ws://y"), "y");
        assert_eq!(strip_ws_scheme("plain"), "plain");
    }
}
