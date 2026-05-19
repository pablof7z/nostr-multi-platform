//! Unit tests for the `bunker://` parser.
//!
//! The 1000-URI fuzz suite lives at `tests/bunker_uri_fuzz.rs` as an integration
//! test so it can stress the public API surface only.

use super::parser::*;

const PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn parses_minimal_bunker_uri() {
    let uri = format!("bunker://{PK}?relay=wss://relay.example.com");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.remote_pubkey_hex, PK);
    assert_eq!(parsed.relays, vec!["wss://relay.example.com".to_string()]);
    assert!(parsed.secret.is_none());
    assert!(parsed.permissions.is_none());
    assert!(parsed.extra.is_empty());
}

#[test]
fn parses_full_bunker_uri() {
    let uri = format!(
        "bunker://{PK}?relay=wss://a.example&relay=wss://b.example&secret=abc123&perms=sign_event:1,nip04_encrypt"
    );
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.relays.len(), 2);
    assert_eq!(parsed.secret.as_deref().map(String::as_str), Some("abc123"));
    assert_eq!(parsed.permissions.as_deref(), Some("sign_event:1,nip04_encrypt"));
}

#[test]
fn preserves_extra_query_params() {
    let uri = format!("bunker://{PK}?relay=wss://r.example&foo=bar&baz=qux");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.extra.len(), 2);
    assert_eq!(parsed.extra[0], ("foo".to_string(), "bar".to_string()));
    assert_eq!(parsed.extra[1], ("baz".to_string(), "qux".to_string()));
}

#[test]
fn rejects_empty() {
    assert_eq!(parse_bunker_uri(""), Err(BunkerParseError::Empty));
}

#[test]
fn rejects_too_long() {
    let suffix = "x".repeat(MAX_BUNKER_URI_LEN);
    let uri = format!("bunker://{PK}?relay=wss://r.example&pad={suffix}");
    match parse_bunker_uri(&uri) {
        Err(BunkerParseError::TooLong(_)) => {}
        other => panic!("expected TooLong, got {other:?}"),
    }
}

#[test]
fn rejects_wrong_scheme() {
    assert!(matches!(
        parse_bunker_uri(&format!("nostr://{PK}?relay=wss://r.example")),
        Err(BunkerParseError::WrongScheme(_))
    ));
    assert!(matches!(
        parse_bunker_uri("https://example.com"),
        Err(BunkerParseError::WrongScheme(_))
    ));
}

#[test]
fn accepts_mixed_case_scheme() {
    // "Bunker://..." should still parse.
    let uri = format!("Bunker://{PK}?relay=wss://r.example");
    assert!(parse_bunker_uri(&uri).is_ok());
}

#[test]
fn rejects_invalid_pubkey() {
    // 63 chars
    assert!(matches!(
        parse_bunker_uri("bunker://abc?relay=wss://r.example"),
        Err(BunkerParseError::InvalidPubkey(_))
    ));
    // non-hex
    let bad_pk: String = "z".repeat(64);
    assert!(matches!(
        parse_bunker_uri(&format!("bunker://{bad_pk}?relay=wss://r.example")),
        Err(BunkerParseError::InvalidPubkey(_))
    ));
}

#[test]
fn requires_at_least_one_relay() {
    assert_eq!(
        parse_bunker_uri(&format!("bunker://{PK}")),
        Err(BunkerParseError::NoRelay)
    );
    assert_eq!(
        parse_bunker_uri(&format!("bunker://{PK}?secret=foo")),
        Err(BunkerParseError::NoRelay)
    );
}

#[test]
fn rejects_invalid_relay_scheme() {
    assert!(matches!(
        parse_bunker_uri(&format!("bunker://{PK}?relay=https://r.example")),
        Err(BunkerParseError::InvalidRelay(_))
    ));
    assert!(matches!(
        parse_bunker_uri(&format!("bunker://{PK}?relay=just-a-host")),
        Err(BunkerParseError::InvalidRelay(_))
    ));
}

#[test]
fn deduplicates_repeated_relays() {
    let uri = format!(
        "bunker://{PK}?relay=wss://a.example&relay=wss://a.example&relay=wss://b.example"
    );
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.relays, vec!["wss://a.example", "wss://b.example"]);
}

#[test]
fn handles_percent_encoded_secret() {
    let uri = format!("bunker://{PK}?relay=wss://r.example&secret=hello%20world");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(
        parsed.secret.as_deref().map(String::as_str),
        Some("hello world")
    );
}

#[test]
fn handles_percent_encoded_relay() {
    let uri = format!(
        "bunker://{PK}?relay=wss%3A%2F%2Fr.example%2Fpath"
    );
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.relays, vec!["wss://r.example/path"]);
}

#[test]
fn accepts_trailing_slash_after_pubkey() {
    let uri = format!("bunker://{PK}/?relay=wss://r.example");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.remote_pubkey_hex, PK);
}

#[test]
fn ignores_empty_query_pairs() {
    let uri = format!("bunker://{PK}?&relay=wss://r.example&&secret=foo&");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.relays.len(), 1);
    assert_eq!(parsed.secret.as_deref().map(String::as_str), Some("foo"));
}

#[test]
fn round_trips_via_display() {
    let uri = format!(
        "bunker://{PK}?relay=wss://r.example&secret=abc&perms=sign_event:1"
    );
    let parsed = parse_bunker_uri(&uri).unwrap();
    let printed = parsed.to_string();
    let reparsed = parse_bunker_uri(&printed).unwrap();
    assert_eq!(parsed, reparsed);
}

#[test]
fn normalises_pubkey_to_lowercase() {
    let upper_pk: String = PK.chars().map(|c| c.to_ascii_uppercase()).collect();
    let uri = format!("bunker://{upper_pk}?relay=wss://r.example");
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.remote_pubkey_hex, PK);
}
