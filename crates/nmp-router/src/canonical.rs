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
//! before calling.
//!
//! ## Single normalization authority + fail-closed (#967)
//!
//! The canonicalization *rules* are NOT re-implemented here — they live in the
//! one workspace-wide authority [`nmp_core::substrate::canonicalize_relay_url`]
//! (the substrate routing layer). This module is a thin scheme-gated adapter
//! over that authority so the router can never drift from the kernel's canonical
//! form.
//!
//! It is **fail-closed**: a URL that passes the caller's cheap `starts_with`
//! gate but is not actually a canonicalizable `wss://` URL (a bare `wss://` or
//! `wss:///path` with no host) returns `None`, and every caller drops/rejects
//! it rather than admitting a malformed key into a routing cache. (The previous
//! fail-open fallback re-admitted exactly those malformed keys.)

/// Canonicalise a `wss://` relay URL: lowercase scheme + host, strip the
/// empty-path trailing slash. Returns `None` (fail-closed) when the authority
/// rejects the URL (no host); the caller MUST drop / reject it.
///
/// Delegates to the single authority [`nmp_core::substrate::canonicalize_relay_url`].
#[must_use]
pub(crate) fn canonicalize_relay_url(url: &str) -> Option<String> {
    debug_assert!(
        url.starts_with("wss://"),
        "canonicalize_relay_url expects a wss:// URL"
    );
    nmp_core::substrate::canonicalize_relay_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_host() {
        assert_eq!(
            canonicalize_relay_url("wss://Block.Example").as_deref(),
            Some("wss://block.example")
        );
    }

    #[test]
    fn strips_empty_path_trailing_slash() {
        assert_eq!(
            canonicalize_relay_url("wss://relay.example/").as_deref(),
            Some("wss://relay.example")
        );
    }

    #[test]
    fn lowercases_host_and_strips_slash_together() {
        assert_eq!(
            canonicalize_relay_url("wss://RELAY.EXAMPLE/").as_deref(),
            Some("wss://relay.example")
        );
    }

    #[test]
    fn preserves_non_empty_path_verbatim() {
        assert_eq!(
            canonicalize_relay_url("wss://Relay.Example/SomePath").as_deref(),
            Some("wss://relay.example/SomePath")
        );
    }

    #[test]
    fn idempotent_on_already_canonical() {
        let once = canonicalize_relay_url("wss://relay.example").expect("canonical");
        assert_eq!(
            canonicalize_relay_url(&once).as_deref(),
            Some(once.as_str())
        );
    }

    #[test]
    fn fail_closed_on_hostless_wss() {
        // Passes the caller's cheap `starts_with("wss://")` gate but has no
        // host — the authority rejects it and we drop it (#967).
        assert_eq!(canonicalize_relay_url("wss://"), None);
        assert_eq!(canonicalize_relay_url("wss:///path"), None);
    }
}
