//! Shared fixtures for the NIP-tag conformance suite: tag-inspection helpers
//! (key-based, order-independent) and the deterministic signed-in harness
//! constructor every scenario module builds on.

use nmp_core::testing::ConformanceHarness;
use serde_json::Value;

/// Deterministic test identity. Same fixture key the in-crate command tests use.
pub(crate) const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// NIP-65 write relays seeded for the active account so the (fail-closed)
/// outbox resolver has targets and publish commands produce outbound frames.
pub(crate) const WRITE_RELAYS: &[&str] =
    &["wss://conformance-w1.test", "wss://conformance-w2.test"];

// ── Tag inspection helpers — key-based, order-independent ───────────────────

/// The `tags` array of an EVENT JSON object, as `Vec<Vec<String>>`.
pub(crate) fn tags_of(event: &Value) -> Vec<Vec<String>> {
    event["tags"]
        .as_array()
        .expect("event has a `tags` array")
        .iter()
        .map(|tag| {
            tag.as_array()
                .expect("each tag is an array")
                .iter()
                .map(|col| col.as_str().expect("tag column is a string").to_string())
                .collect()
        })
        .collect()
}

/// Every tag whose first column equals `key` (e.g. all `e` tags). Order of the
/// returned tags mirrors the wire, but callers must not depend on it.
pub(crate) fn tags_with_key(event: &Value, key: &str) -> Vec<Vec<String>> {
    tags_of(event)
        .into_iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .collect()
}

/// The value column (index 1) of every tag with first column `key`.
pub(crate) fn values_for_key(event: &Value, key: &str) -> Vec<String> {
    tags_with_key(event, key)
        .into_iter()
        .filter_map(|t| t.get(1).cloned())
        .collect()
}

/// The distinct set of tag keys present on the event (first column of each tag).
pub(crate) fn distinct_keys(event: &Value) -> Vec<String> {
    let mut keys: Vec<String> = tags_of(event)
        .into_iter()
        .filter_map(|t| t.into_iter().next())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Assert the event carries tags from exactly `allowed` keys and no others —
/// the "no forbidden or surprising tags snuck in" half of conformance.
pub(crate) fn assert_only_keys(event: &Value, allowed: &[&str], context: &str) {
    let mut expected: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    expected.sort();
    expected.dedup();
    assert_eq!(
        distinct_keys(event),
        expected,
        "{context}: event carries an unexpected tag key (or is missing one)"
    );
}

/// A 64-char hex pubkey/event-id literal built from one repeated nibble.
pub(crate) fn hex64(nibble: char) -> String {
    std::iter::repeat_n(nibble, 64).collect()
}

/// True if `s` is a 64-char lowercase-hex string (event-id / pubkey shape).
pub(crate) fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn signed_harness() -> ConformanceHarness {
    let mut h = ConformanceHarness::new();
    h.sign_in_and_seed_nip65(TEST_NSEC, WRITE_RELAYS);
    h
}
