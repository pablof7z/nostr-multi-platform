//! Relay-URL recognition + normalization (issue #1804).
//!
//! Detects a `ws://` / `wss://` relay URL in the raw input and normalizes it
//! (lowercase scheme/host, trailing-slash policy) so the classifier can emit a
//! canonical `InputIntentTarget::RelayUrl`. SHAPE/parse only — never connects.
//!
//! The normalization rules are NOT re-implemented here: this delegates to the
//! single relay-URL authority [`nmp_core::substrate::canonicalize_relay_url`]
//! (the `nmp-relay-url` Layer-0 crate), so the classifier's canonical form is
//! byte-identical to what the planner / network / router dial and persist. A URL
//! the user blocked under one spelling can therefore never re-enter through this
//! recognizer under a different spelling (#967).

/// Recognize a `ws://` / `wss://` relay URL and return its canonical form.
///
/// Returns `None` when the input is not a `ws`/`wss` URL or cannot be
/// canonicalized (fail-closed — mirrors the authority's contract). SHAPE/parse
/// only: never connects, never does IO.
///
/// The scheme check is performed on the trimmed input case-insensitively before
/// delegating, so only relay URLs reach the authority — a bare `http(s)` link or
/// free text returns `None` and falls through to the next precedence rung.
#[must_use]
pub fn recognize_relay_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if !has_ws_scheme(trimmed) {
        return None;
    }
    nmp_core::substrate::canonicalize_relay_url(trimmed)
}

/// True iff `input` starts with a `ws://` or `wss://` scheme (ASCII
/// case-insensitive). Only the scheme is inspected; full validity is the
/// authority's job.
fn has_ws_scheme(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.starts_with("ws://") || lower.starts_with("wss://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wss_url_is_recognized_and_canonicalized() {
        let got = recognize_relay_url("wss://relay.example.com").expect("relay");
        // The authority lowercases + applies its trailing-slash policy; assert it
        // round-trips back through itself (idempotent canonical form).
        assert_eq!(
            Some(got.clone()),
            nmp_core::substrate::canonicalize_relay_url(&got)
        );
        assert!(got.starts_with("wss://"));
    }

    #[test]
    fn ws_url_is_recognized() {
        assert!(recognize_relay_url("ws://localhost:7777").is_some());
    }

    #[test]
    fn scheme_match_is_case_insensitive() {
        assert!(recognize_relay_url("WSS://Relay.Example.COM").is_some());
    }

    #[test]
    fn leading_and_trailing_whitespace_is_tolerated() {
        assert!(recognize_relay_url("  wss://relay.example.com  ").is_some());
    }

    #[test]
    fn http_url_is_not_a_relay() {
        assert_eq!(recognize_relay_url("https://relay.example.com"), None);
    }

    #[test]
    fn free_text_is_not_a_relay() {
        assert_eq!(recognize_relay_url("just some words"), None);
    }

    #[test]
    fn ws_scheme_without_authority_fails_closed() {
        // `ws://` with no host cannot be canonicalized → None (fail-closed),
        // never a half-formed RelayUrl.
        assert_eq!(recognize_relay_url("ws://"), None);
    }
}
