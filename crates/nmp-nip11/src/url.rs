//! Relay URL → NIP-11 HTTP URL mapping.
//!
//! NIP-11 is fetched over HTTP(S) on the same host/path as the relay's
//! WebSocket URL: `wss://` maps to `https://`, `ws://` maps to `http://`. A URL
//! that is already `http(s)://` is returned unchanged (some callers preview a
//! relay by its HTTP form). Anything else (a bare host, an unknown scheme)
//! yields `None` — the fetch is skipped rather than guessed.

/// Map a relay WebSocket URL to the HTTP URL its NIP-11 document is served from.
///
/// - `wss://relay.example/path` → `https://relay.example/path`
/// - `ws://relay.example`       → `http://relay.example`
/// - `https://relay.example`    → `https://relay.example` (unchanged)
/// - `http://relay.example`     → `http://relay.example` (unchanged)
/// - anything else              → `None`
///
/// Leading/trailing ASCII whitespace is trimmed. The scheme match is
/// case-insensitive (`WSS://` works); the rest of the URL is preserved verbatim.
#[must_use]
pub fn http_url_for_relay(relay_url: &str) -> Option<String> {
    let trimmed = relay_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Case-insensitive scheme detection without allocating a lowercased copy of
    // the whole URL (the path may be case-sensitive).
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("wss://") {
        return Some(format!(
            "https://{}",
            &trimmed[trimmed.len() - rest.len()..]
        ));
    }
    if let Some(rest) = lower.strip_prefix("ws://") {
        return Some(format!("http://{}", &trimmed[trimmed.len() - rest.len()..]));
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(trimmed.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wss_maps_to_https() {
        assert_eq!(
            http_url_for_relay("wss://relay.example"),
            Some("https://relay.example".to_string())
        );
    }

    #[test]
    fn ws_maps_to_http() {
        assert_eq!(
            http_url_for_relay("ws://relay.example"),
            Some("http://relay.example".to_string())
        );
    }

    #[test]
    fn path_and_query_are_preserved() {
        assert_eq!(
            http_url_for_relay("wss://relay.example/nostr?x=1"),
            Some("https://relay.example/nostr?x=1".to_string())
        );
    }

    #[test]
    fn scheme_match_is_case_insensitive() {
        assert_eq!(
            http_url_for_relay("WSS://Relay.Example/Path"),
            Some("https://Relay.Example/Path".to_string())
        );
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(
            http_url_for_relay("  wss://relay.example  "),
            Some("https://relay.example".to_string())
        );
    }

    #[test]
    fn http_urls_pass_through_unchanged() {
        assert_eq!(
            http_url_for_relay("https://relay.example"),
            Some("https://relay.example".to_string())
        );
        assert_eq!(
            http_url_for_relay("http://relay.example"),
            Some("http://relay.example".to_string())
        );
    }

    #[test]
    fn unknown_schemes_and_empty_yield_none() {
        assert_eq!(http_url_for_relay(""), None);
        assert_eq!(http_url_for_relay("   "), None);
        assert_eq!(http_url_for_relay("relay.example"), None);
        assert_eq!(http_url_for_relay("ftp://relay.example"), None);
    }
}
