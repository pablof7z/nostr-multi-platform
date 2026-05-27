use super::{canonical_relay_url, CanonicalRelayUrl};

#[test]
fn t_canonicalize_lowercase_scheme_and_host() {
    assert_eq!(
        canonical_relay_url("WSS://R.Ex"),
        Some("wss://r.ex".to_string()),
        "scheme and host must be lowercased"
    );
}

#[test]
fn t_canonicalize_strip_empty_path_trailing_slash() {
    assert_eq!(
        canonical_relay_url("wss://r.ex/"),
        Some("wss://r.ex".to_string()),
        "trailing slash on empty path must be stripped"
    );
}

#[test]
fn t_canonicalize_case_and_trailing_slash_combined() {
    assert_eq!(
        canonical_relay_url("WSS://R.Ex/"),
        Some("wss://r.ex".to_string()),
        "uppercase scheme+host AND empty-path trailing slash"
    );
}

#[test]
fn t_canonicalize_preserve_nonempty_path() {
    assert_eq!(
        canonical_relay_url("wss://r.ex/nostr"),
        Some("wss://r.ex/nostr".to_string()),
        "non-empty path must be preserved"
    );
}

#[test]
fn t_canonicalize_preserve_nonempty_path_with_trailing_slash() {
    // A relay with a real path retains its trailing slash.
    assert_eq!(
        canonical_relay_url("wss://r.ex/nostr/"),
        Some("wss://r.ex/nostr/".to_string()),
        "trailing slash on non-empty path must be preserved"
    );
}

#[test]
fn t_canonicalize_path_distinctness() {
    // A relay with a real path is distinct from the no-path form.
    let with_path = canonical_relay_url("wss://r.ex/nostr");
    let no_path = canonical_relay_url("wss://r.ex");
    assert_ne!(
        with_path, no_path,
        "wss://r.ex/nostr must be distinct from wss://r.ex"
    );
}

#[test]
fn t_canonicalize_preserve_port() {
    assert_eq!(
        canonical_relay_url("wss://r.ex:7777/"),
        Some("wss://r.ex:7777".to_string()),
        "port must be preserved, empty-path slash stripped"
    );
}

#[test]
fn t_canonicalize_preserve_query() {
    assert_eq!(
        canonical_relay_url("WSS://R.Ex?foo=bar"),
        Some("wss://r.ex?foo=bar".to_string()),
        "query string must be preserved, scheme+host lowercased"
    );
}

#[test]
fn t_canonicalize_ws_scheme() {
    assert_eq!(
        canonical_relay_url("ws://r.ex/"),
        Some("ws://r.ex".to_string()),
        "ws:// scheme is valid"
    );
}

#[test]
fn t_canonicalize_reject_http() {
    assert_eq!(
        canonical_relay_url("http://r.ex"),
        None,
        "http scheme must be rejected"
    );
}

#[test]
fn t_canonicalize_reject_https() {
    assert_eq!(
        canonical_relay_url("https://r.ex"),
        None,
        "https scheme must be rejected"
    );
}

#[test]
fn t_canonicalize_reject_empty() {
    assert_eq!(
        canonical_relay_url(""),
        None,
        "empty string must be rejected"
    );
}

#[test]
fn t_canonicalize_trims_whitespace() {
    assert_eq!(
        canonical_relay_url("  wss://r.ex/  "),
        // Note: only leading/trailing whitespace is stripped from the raw
        // input. The trailing "  " is after the full URL so it's part of
        // path_etc — we do NOT strip inner whitespace. In practice relay
        // URLs do not contain embedded spaces, and `trim()` on the whole
        // input handles the common FFI/copy-paste case.
        // After trim → "wss://r.ex/" → empty path → strip slash.
        Some("wss://r.ex".to_string()),
        "leading/trailing whitespace must be stripped"
    );
}

// ── CanonicalRelayUrl newtype ────────────────────────────────────────────

#[test]
fn t_newtype_parse_canonicalizes() {
    let url = CanonicalRelayUrl::parse("WSS://R.Ex/").expect("ws/wss URL must parse");
    assert_eq!(
        url.as_str(),
        "wss://r.ex",
        "parse must canonicalize scheme/host and strip empty-path slash"
    );
}

#[test]
fn t_newtype_parse_rejects_bad_scheme() {
    assert!(
        CanonicalRelayUrl::parse("http://r.ex").is_none(),
        "non-ws/wss scheme must not produce a CanonicalRelayUrl"
    );
}

#[test]
fn t_newtype_equal_spellings_collapse_to_one_key() {
    // The whole point of the newtype: two spellings of the same relay
    // canonicalize to a single, equal value — so they index one map row.
    let a = CanonicalRelayUrl::parse("wss://Relay.MIXED/").unwrap();
    let b = CanonicalRelayUrl::parse("WSS://relay.mixed").unwrap();
    assert_eq!(a, b, "URL-equivalent inputs must compare equal");
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let hash = |u: &CanonicalRelayUrl| {
        let mut h = DefaultHasher::new();
        u.hash(&mut h);
        h.finish()
    };
    assert_eq!(
        hash(&a),
        hash(&b),
        "equal values must hash equal (HashMap key)"
    );
}

#[test]
fn t_newtype_parse_or_raw_fails_open_for_bad_input() {
    // `parse_or_raw` preserves the pre-newtype fail-open contract: a
    // malformed URL is wrapped verbatim so a lookup against the identical
    // malformed input still matches.
    let raw = CanonicalRelayUrl::parse_or_raw("not-a-url");
    assert_eq!(raw.as_str(), "not-a-url");
    let same = CanonicalRelayUrl::parse_or_raw("not-a-url");
    assert_eq!(
        raw, same,
        "fail-open keys for the same raw input must match"
    );
}

#[test]
fn t_newtype_string_comparison_helpers() {
    let url = CanonicalRelayUrl::parse("wss://r.ex").unwrap();
    assert_eq!(url, "wss://r.ex", "PartialEq<&str> must work");
    assert!(url.starts_with("wss://"), "Deref<str> exposes &str methods");
}
