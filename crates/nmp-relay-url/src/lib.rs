//! Relay-URL canonicalization — the **single** workspace authority (Layer 0).
//!
//! A relay URL is the key three+ subsystems hand each other (the transport
//! pool, the routing/mailbox caches, the blocked-relay filter). A REQ
//! registered under one spelling (`wss://Relay.MIXED/`) and an EOSE or block
//! entry delivered under another (`wss://relay.mixed`) MUST hit the same row —
//! so every consumer has to agree on one canonical form. Before this crate
//! existed there were *five* independent copies of these rules scattered across
//! `nmp-core`, `nmp-router`, `nmp-planner`, `nmp-nip17`, and a test helper, and
//! they had drifted in scheme coverage and fail-open vs fail-closed behavior —
//! a relay the user blocked under one spelling could still receive their
//! traffic under another (#967, a privacy regression).
//!
//! This crate is dependency-free and sits at Layer 0 (vocabulary), so every
//! layer that needs the rules can depend on it without a layering inversion:
//! `nmp-network` (L1), `nmp-router` / `nmp-planner` (L2), `nmp-core` (L3), and
//! the protocol crates (L4) all delegate here. There is exactly one
//! implementation of the rules and it lives in [`canonicalize`].

/// Canonicalize a relay URL into its single canonical form, or `None` when the
/// URL is not a canonicalizable `ws`/`wss` relay URL (**fail-closed** — the
/// caller MUST NOT dial / persist / route to a relay it cannot canonicalize).
///
/// # Rules (per URL semantics + NIP-01 relay URL conventions)
/// - Lowercase scheme and authority (host[:port]).
/// - Strip a single trailing `/` **only when the path is empty**
///   (`wss://r.ex/` → `wss://r.ex`); non-empty paths are preserved verbatim
///   including any trailing slash (`wss://r.ex/nostr/` is unchanged).
/// - Reject any URL whose scheme is not `ws` or `wss` (after lowercasing).
/// - Preserve path, query, and fragment exactly as given (only scheme+host are
///   lowercased).
/// - Leading/trailing ASCII whitespace is stripped before parsing.
#[must_use]
pub fn canonicalize(raw: &str) -> Option<String> {
    let s = raw.trim();
    // Find the scheme separator "://".
    let sep = s.find("://")?;
    let scheme = s[..sep].to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return None;
    }
    // Everything after "://" — split authority from path+query+fragment.
    let rest = &s[sep + 3..];
    if rest.is_empty() {
        return None; // no authority
    }
    // Authority ends at the first '/', '?', or '#'.
    let (authority, path_etc) = if let Some(pos) = rest.find(['/', '?', '#']) {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };
    if authority.is_empty() {
        return None; // e.g. "wss:///path" — no host
    }
    let authority_lower = authority.to_ascii_lowercase();
    // Strip trailing '/' only when path is exactly "/" (empty path notation).
    let path_etc_norm = if path_etc == "/" { "" } else { path_etc };
    Some(format!("{scheme}://{authority_lower}{path_etc_norm}"))
}

#[cfg(test)]
mod tests {
    use super::canonicalize;

    #[test]
    fn lowercases_scheme_and_host() {
        assert_eq!(canonicalize("WSS://R.Ex").as_deref(), Some("wss://r.ex"));
    }

    #[test]
    fn strips_empty_path_trailing_slash() {
        assert_eq!(canonicalize("wss://r.ex/").as_deref(), Some("wss://r.ex"));
    }

    #[test]
    fn lowercase_and_strip_together() {
        assert_eq!(canonicalize("WSS://R.Ex/").as_deref(), Some("wss://r.ex"));
    }

    #[test]
    fn preserves_non_empty_path_verbatim() {
        assert_eq!(
            canonicalize("wss://r.ex/nostr").as_deref(),
            Some("wss://r.ex/nostr"),
        );
        assert_eq!(
            canonicalize("wss://r.ex/nostr/").as_deref(),
            Some("wss://r.ex/nostr/"),
            "a non-empty path's trailing slash is preserved",
        );
    }

    #[test]
    fn preserves_port_and_query() {
        assert_eq!(
            canonicalize("wss://r.ex:7777/").as_deref(),
            Some("wss://r.ex:7777"),
        );
        assert_eq!(
            canonicalize("WSS://R.Ex?foo=bar").as_deref(),
            Some("wss://r.ex?foo=bar"),
        );
    }

    #[test]
    fn accepts_ws_scheme() {
        assert_eq!(canonicalize("ws://r.ex/").as_deref(), Some("ws://r.ex"));
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            canonicalize("  wss://r.ex/  ").as_deref(),
            Some("wss://r.ex"),
        );
    }

    #[test]
    fn fail_closed_on_bad_scheme() {
        assert_eq!(canonicalize("http://r.ex"), None);
        assert_eq!(canonicalize("https://r.ex"), None);
    }

    #[test]
    fn fail_closed_on_missing_authority() {
        assert_eq!(canonicalize("wss://"), None);
        assert_eq!(canonicalize("wss:///path"), None);
        assert_eq!(canonicalize(""), None);
    }

    #[test]
    fn idempotent_on_already_canonical() {
        let once = canonicalize("wss://relay.example").expect("canonical");
        assert_eq!(canonicalize(&once).as_deref(), Some(once.as_str()));
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
