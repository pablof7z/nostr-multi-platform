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

// ─── NIP-02 kind:3 contact-list edit builders ────────────────────────────────

/// Return the FULL kind:3 tag set that results from adding a follow on `target`
/// to `current` — splicing ONLY the `p` section while preserving everything
/// else verbatim (issue #1246).
///
/// `current` is the active account's existing kind:3 tag set
/// (`Vec<Vec<String>>`), obtained from a confirmed-loaded kind:3 via
/// [`crate::kernel_reducer::KernelReducer::try_current_contact_list_event`] (or
/// the native kernel contact-list edit gate). Callers MUST confirm the kind:3
/// is loaded first — editing a not-yet-loaded list and re-publishing would
/// silently wipe the user's contacts.
///
/// Preservation contract:
/// - Every **non-`p`** tag (relay-list `["r", …]`, `["d", …]`, etc.) is
///   carried through verbatim, in document order.
/// - Every existing `["p", pk, relay?, petname?]` entry keeps its relay-hint
///   (column 2) and petname (column 3) columns — the edit never strips them.
/// - Document order of all retained tags is preserved.
///
/// Idempotent: if a `p` tag for `target` (matched on column 1, the pubkey) is
/// already present, the set is returned unchanged — no duplicate, and the
/// existing entry's relay-hint / petname survive. Otherwise a bare
/// `["p", target]` is appended after the existing tags.
#[must_use]
pub fn kind3_tags_after_add(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    let mut tags: Vec<Vec<String>> = current.to_vec();
    let already_present = tags.iter().any(|t| {
        t.first().map(String::as_str) == Some("p") && t.get(1).map(String::as_str) == Some(target)
    });
    if !already_present {
        tags.push(vec!["p".to_string(), target.to_string()]);
    }
    tags
}

/// Return the FULL kind:3 tag set that results from removing the follow on
/// `target` from `current` — dropping ONLY the matching `p` entries while
/// preserving everything else verbatim (issue #1246).
///
/// Drops every `["p", target, …]` entry of ANY arity (bare, relay-hinted, or
/// relay-hinted-with-petname) matched on column 1 (the pubkey). Every non-`p`
/// tag and every `p` tag for a different pubkey — including its relay-hint and
/// petname columns — is carried through verbatim in document order.
///
/// Idempotent: if no `p` tag for `target` is present, the set is returned
/// unchanged. Same must-be-loaded safety constraint as
/// [`kind3_tags_after_add`].
#[must_use]
pub fn kind3_tags_after_remove(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    current
        .iter()
        .filter(|t| {
            !(t.first().map(String::as_str) == Some("p")
                && t.get(1).map(String::as_str) == Some(target))
        })
        .cloned()
        .collect()
}

// ─── NIP-25 reaction builder ─────────────────────────────────────────────────

/// Build NIP-25 kind:7 reaction tags and normalised content for
/// `target_event_id`.
///
/// Returns `None` when `target_event_id` is not a valid 64-char hex event id
/// (same gate as `crate::kernel::is_hex_id`). Otherwise returns
/// `Some((tags, content))` where:
/// - `tags` = `[["e", target_event_id], ["p", author]?]`
/// - `content` = `reaction` normalised to `"+"` when blank
///
/// `author` is `None` when the target event's author is absent from the
/// caller's read-cache; the e-tag-only reaction is still valid NIP-25 (D6:
/// degrade, never refuse the publish).
///
/// Shared canonical implementation; both `KernelReducer::build_reaction_draft`
/// (wasm write-path) and native `actor::commands::publish::react` delegate
/// here so tag logic is defined once and cannot silently drift.
#[must_use]
pub fn reaction_tags(
    target_event_id: &str,
    author: Option<&str>,
    reaction: &str,
) -> Option<(Vec<Vec<String>>, String)> {
    if !crate::kernel::is_hex_id(target_event_id) {
        return None;
    }
    let content = if reaction.trim().is_empty() {
        "+".to_string()
    } else {
        reaction.to_string()
    };
    let mut tags = vec![vec!["e".to_string(), target_event_id.to_string()]];
    if let Some(pk) = author {
        tags.push(vec!["p".to_string(), pk.to_string()]);
    }
    Some((tags, content))
}

// Unit tests live in a sibling file to keep this module under the 500-line
// ceiling. `tags_tests.rs` covers constructors, readers, and kind-3 edit tags.
#[cfg(test)]
#[path = "tags_tests.rs"]
mod tags_tests;
