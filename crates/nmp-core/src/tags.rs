//! Shared, kind-agnostic Nostr tag helpers.
//!
//! Per-kind and per-NIP grammars live in their protocol crates. For example,
//! NIP-10 kind:1 reply parsing/building is owned by `nmp-nip01`.

// ─── Tag constructors ────────────────────────────────────────────────────────

/// Build an `e` tag: `["e", <id>]`, optionally with a relay hint and a
/// NIP-10 marker (`"root"` / `"reply"` / `"mention"`).
///
/// NIP-10 marked form requires the relay slot to be present (possibly empty)
/// when a marker follows, so a `Some(marker)` always emits the 4-column form
/// `["e", id, relay_or_empty, marker]`.
#[must_use]
pub fn e_tag(id: &str, relay: Option<&str>, marker: Option<&str>) -> Vec<String> {
    match (relay, marker) {
        (_, Some(marker)) => vec![
            "e".to_string(),
            id.to_string(),
            relay.unwrap_or("").to_string(),
            marker.to_string(),
        ],
        (Some(relay), None) => vec!["e".to_string(), id.to_string(), relay.to_string()],
        (None, None) => vec!["e".to_string(), id.to_string()],
    }
}

/// Build a `p` tag: `["p", <pubkey>]`, optionally with a relay hint.
#[must_use]
pub fn p_tag(pubkey: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["p".to_string(), pubkey.to_string(), relay.to_string()],
        None => vec!["p".to_string(), pubkey.to_string()],
    }
}

/// Build a NIP-33 `a` tag: `["a", "<kind>:<pubkey>:<d_tag>"]`, optionally with
/// a relay hint.
#[must_use]
pub fn a_tag(kind: u32, pubkey: &str, d_tag: &str, relay: Option<&str>) -> Vec<String> {
    let coord = format!("{kind}:{pubkey}:{d_tag}");
    match relay {
        Some(relay) => vec!["a".to_string(), coord, relay.to_string()],
        None => vec!["a".to_string(), coord],
    }
}

/// Build a NIP-18 `q` (quote) tag: `["q", <id>]`, optionally with a relay hint.
#[must_use]
pub fn q_tag(id: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["q".to_string(), id.to_string(), relay.to_string()],
        None => vec!["q".to_string(), id.to_string()],
    }
}

// ─── Tag readers ─────────────────────────────────────────────────────────────

/// Return the second column of the first tag whose first column equals `key`.
///
/// Promoted here from the copy that was private to `nmp-nip23::decode` so
/// every protocol crate shares one implementation.
#[must_use]
pub fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

/// Return the second column of every tag whose first column equals `key`,
/// in document order.
#[must_use]
pub fn all_tag_values<'a>(tags: &'a [Vec<String>], key: &str) -> Vec<&'a str> {
    tags.iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .filter_map(|t| t.get(1))
        .map(String::as_str)
        .collect()
}

// Unit tests live in a sibling file to keep this module under the 500-line
// ceiling. `tags_tests.rs` covers the shared constructors and readers.
#[cfg(test)]
#[path = "tags_tests.rs"]
mod tags_tests;
