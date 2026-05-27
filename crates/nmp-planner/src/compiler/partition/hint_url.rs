//! URL validation and normalization for relay hints.
//!
//! `is_valid_hint_url` mirrors `nmp-core::canonical_relay_url` locally so that
//! the planner (Layer 2) does not depend on Layer 3 (`nmp-core`) for hint
//! validation.  The rules are identical; changes to the core function must be
//! mirrored here.
//!
//! Normalization rules (identical to `nmp_core::relay::CanonicalRelayUrl::parse`):
//! - Accept `ws://` or `wss://` scheme (case-insensitive).
//! - Strip leading/trailing ASCII whitespace from the raw input.
//! - Require a non-empty authority (after stripping the scheme).
//! - Lowercase the scheme and authority (host + optional port).
//! - Strip a single trailing `/` **only when the path is empty**
//!   (`wss://r.ex/` → `wss://r.ex`; `wss://r.ex/nostr/` is unchanged).
//! - Preserve path, query, and fragment exactly as given.
//!
//! The normalized string is returned so it can be used directly as the
//! `BTreeMap` key in the hint-lane accumulator — callers must NOT use the raw
//! `hint.url` as the key.

/// Validate and normalize a raw relay-hint URL.
///
/// Returns `Some(normalized)` when the URL is a valid `ws://` or `wss://`
/// URL with a non-empty authority; `None` otherwise.
///
/// The returned string is the canonical form used as the relay-map key.  Using
/// the raw URL as a key would prevent deduplication against NIP-65 routes that
/// were canonicalized upstream.
pub(super) fn is_valid_hint_url(raw: &str) -> Option<String> {
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
        return None; // e.g. "wss:///path" or "wss://?query" — no host
    }
    let authority_lower = authority.to_ascii_lowercase();
    // Strip trailing '/' only when path is exactly "/" (empty path notation).
    let path_etc_norm = if path_etc == "/" { "" } else { path_etc };
    Some(format!("{scheme}://{authority_lower}{path_etc_norm}"))
}

#[cfg(test)]
mod tests {
    use super::is_valid_hint_url;

    /// wss:// with trailing slash on empty path is normalized (dedup case).
    #[test]
    fn trailing_slash_stripped_for_empty_path() {
        assert_eq!(
            is_valid_hint_url("wss://relay.example/"),
            Some("wss://relay.example".to_string()),
            "trailing empty-path slash must be stripped so the key matches upstream-canonical NIP-65 form"
        );
    }

    /// Mixed-case scheme and host are lowercased.
    #[test]
    fn case_insensitive_scheme_and_host() {
        assert_eq!(
            is_valid_hint_url("WSS://Relay.EXAMPLE"),
            Some("wss://relay.example".to_string()),
            "scheme and host must be lowercased"
        );
    }

    /// Missing authority is rejected (`wss:///path` and `wss://?query`).
    #[test]
    fn missing_authority_rejected() {
        assert_eq!(
            is_valid_hint_url("wss:///path"),
            None,
            "wss:///path has empty authority and must be rejected"
        );
        assert_eq!(
            is_valid_hint_url("wss://?query"),
            None,
            "wss://?query has empty authority and must be rejected"
        );
        assert_eq!(
            is_valid_hint_url("wss://"),
            None,
            "bare wss:// with no authority must be rejected"
        );
    }

    /// Both ws:// and wss:// schemes are accepted.
    #[test]
    fn both_schemes_accepted() {
        assert_eq!(
            is_valid_hint_url("wss://relay.example"),
            Some("wss://relay.example".to_string()),
            "wss:// must be accepted"
        );
        // The bug case: ws://x is 6 chars; the old `len() > "wss://".len()` check
        // would produce `6 > 6 == false` and reject it.  We must accept it.
        assert_eq!(
            is_valid_hint_url("ws://x"),
            Some("ws://x".to_string()),
            "ws://x (minimal 1-char host) must be accepted; the old >6 check incorrectly rejected it"
        );
        assert_eq!(
            is_valid_hint_url("ws://relay.example"),
            Some("ws://relay.example".to_string()),
            "ws:// with normal host must be accepted"
        );
    }

    /// Query string is preserved verbatim.
    #[test]
    fn query_string_preserved() {
        assert_eq!(
            is_valid_hint_url("WSS://Relay.EXAMPLE?token=abc"),
            Some("wss://relay.example?token=abc".to_string()),
            "query string must be preserved; only scheme+host are lowercased"
        );
    }

    /// Non-empty path's trailing slash is preserved (not stripped).
    #[test]
    fn nonempty_path_trailing_slash_preserved() {
        assert_eq!(
            is_valid_hint_url("wss://relay.example/nostr/"),
            Some("wss://relay.example/nostr/".to_string()),
            "trailing slash on a non-empty path must not be stripped"
        );
    }

    /// http:// and https:// are rejected.
    #[test]
    fn non_ws_schemes_rejected() {
        assert_eq!(is_valid_hint_url("http://relay.example"), None);
        assert_eq!(is_valid_hint_url("https://relay.example"), None);
    }

    /// Empty string is rejected.
    #[test]
    fn empty_string_rejected() {
        assert_eq!(is_valid_hint_url(""), None);
    }

    /// Leading/trailing whitespace is stripped.
    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            is_valid_hint_url("  wss://relay.example/  "),
            // After trim → "wss://relay.example/" → path "/" → stripped.
            Some("wss://relay.example".to_string()),
        );
    }
}
