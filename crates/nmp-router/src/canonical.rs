//! `canonicalize_relay_url` — the single canonical relay-URL normaliser the
//! routing layer shares across its caches and parsers.
//!
//! Three router-layer caches key on relay URLs and MUST agree on one canonical
//! form so a URL parsed from one source matches a URL parsed from another:
//!
//! - the kind:10006 blocked-relay cache ([`crate::blocked_relays`]),
//! - the kind:10002 NIP-65 mailbox cache ([`crate::ingest`]),
//! - (transitively) any consumer that subtracts a blocked URL from a routed
//!   set.
//!
//! Before this module each parser carried its own copy of the normaliser (or
//! none at all). The kind:10002 parser previously stored URLs verbatim, so a
//! relay listed as `wss://Blocked.Example/` in an author's kind:10002 never
//! matched `wss://blocked.example` in the canonicalised blocked-relay cache —
//! the blocked relay silently leaked back into the author's write set. Routing
//! correctness depends on one shared normaliser; this module is it.
//!
//! Canonical form: lowercase scheme + host, strip the empty-path trailing
//! slash. Paths (some relays are path-scoped, e.g. `wss://relay.example/nostr`)
//! are preserved verbatim — only a bare `/` is dropped.

/// Canonicalise a `wss://` relay URL: lowercase scheme + host, strip the
/// empty-path trailing slash. A non-empty path is preserved verbatim (some
/// relays are path-scoped). Non-`wss://` input is returned unchanged in debug
/// builds the assertion fires; callers gate on the scheme before calling.
#[must_use]
pub(crate) fn canonicalize_relay_url(url: &str) -> String {
    const PREFIX: &str = "wss://";
    debug_assert!(url.starts_with(PREFIX));
    let Some(rest) = url.strip_prefix(PREFIX) else {
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
    use super::canonicalize_relay_url;

    #[test]
    fn lowercases_host_and_strips_trailing_slash() {
        assert_eq!(
            canonicalize_relay_url("wss://Blocked.Example/"),
            "wss://blocked.example"
        );
        assert_eq!(
            canonicalize_relay_url("wss://BLOCKED.EXAMPLE"),
            "wss://blocked.example"
        );
    }

    #[test]
    fn preserves_nonempty_path() {
        assert_eq!(
            canonicalize_relay_url("wss://Relay.Example/nostr"),
            "wss://relay.example/nostr"
        );
    }

    #[test]
    fn already_canonical_is_idempotent() {
        let canonical = "wss://relay.example";
        assert_eq!(canonicalize_relay_url(canonical), canonical);
        assert_eq!(
            canonicalize_relay_url(&canonicalize_relay_url("wss://Relay.Example/")),
            canonical
        );
    }

    #[test]
    fn preserves_port() {
        assert_eq!(
            canonicalize_relay_url("wss://Relay.Example:7777/"),
            "wss://relay.example:7777"
        );
    }
}
