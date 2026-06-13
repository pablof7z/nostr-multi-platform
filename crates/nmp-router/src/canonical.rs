//! `canonicalize_relay_url` — the single canonical-form helper shared by every
//! routing-side relay-URL consumer in `nmp-router`.
//!
//! Two NIP-51-adjacent parsers must agree on one canonical form so their URL
//! keys collide cleanly:
//!
//! - [`crate::ingest`] (kind:10002 NIP-65 relay list → [`crate::InMemoryMailboxCache`])
//! - [`crate::blocked_relays`] (kind:10006 NIP-51 blocked relays → `InMemoryBlockedRelayCache`)
//!
//! Before this module existed, `blocked_relays.rs` canonicalised its URLs
//! (lowercase host, strip empty-path trailing slash) but `ingest.rs` stored
//! NIP-65 URLs verbatim. A blocked entry `wss://Block.Example` therefore never
//! matched a kind:10002 write entry `wss://block.example/` — the blocked
//! filter silently failed (a privacy regression: a relay the user told us to
//! never publish to / subscribe through still received their traffic). Routing
//! both through one function closes that gap structurally.
//!
//! ## Canonical form
//!
//! - scheme + host lowercased (host names are case-insensitive per RFC 3986)
//! - an empty-path trailing slash (`wss://relay.example/`) is stripped to
//!   `wss://relay.example`
//! - a non-empty path is preserved verbatim (case included — paths can be
//!   case-sensitive)
//!
//! Only `wss://` URLs reach here in production; the callers gate the scheme
//! before calling. `debug_assert!` guards the precondition in test/debug
//! builds; a non-`wss://` input in release returns the input unchanged rather
//! than panicking (fail-open, never crash a routing path — D2/D15).

/// Canonicalise a `wss://` relay URL: lowercase scheme + host, strip the
/// empty-path trailing slash. See the module docs for the exact rules.
#[must_use]
pub(crate) fn canonicalize_relay_url(url: &str) -> String {
    const PREFIX: &str = "wss://";
    debug_assert!(
        url.starts_with(PREFIX),
        "canonicalize_relay_url expects a wss:// URL"
    );
    let Some(rest) = url.strip_prefix(PREFIX) else {
        // Release-build fail-open: a non-wss URL slipped past the caller's
        // scheme gate. Return it unchanged rather than panic on a routing path.
        return url.to_string();
    };
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let canonical_host = host_port.to_lowercase();
    let canonical_path = if path == "/" { "" } else { path };
    format!("{PREFIX}{canonical_host}{canonical_path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_host() {
        assert_eq!(
            canonicalize_relay_url("wss://Block.Example"),
            "wss://block.example"
        );
    }

    #[test]
    fn strips_empty_path_trailing_slash() {
        assert_eq!(
            canonicalize_relay_url("wss://relay.example/"),
            "wss://relay.example"
        );
    }

    #[test]
    fn lowercases_host_and_strips_slash_together() {
        assert_eq!(
            canonicalize_relay_url("wss://RELAY.EXAMPLE/"),
            "wss://relay.example"
        );
    }

    #[test]
    fn preserves_non_empty_path_verbatim() {
        assert_eq!(
            canonicalize_relay_url("wss://Relay.Example/SomePath"),
            "wss://relay.example/SomePath"
        );
    }

    #[test]
    fn idempotent_on_already_canonical() {
        let once = canonicalize_relay_url("wss://relay.example");
        assert_eq!(canonicalize_relay_url(&once), once);
    }
}
