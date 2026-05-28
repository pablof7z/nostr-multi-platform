use super::super::StoredEvent;

/// Extract the two fields a kind:6 row needs from the NIP-18 embedded event
/// JSON: the inner event's `id` (for thread navigation) and `content` (for
/// rendering). Returns `(None, None)` when `raw` is not a JSON object or
/// when neither field is a string, mirroring the Swift `innerEventField`
/// helper that this function replaces.
///
/// Pure, allocation-bounded, no I/O — safe to call on every snapshot tick.
/// This is a display-layer extractor owned by the kernel so the Swift
/// thin-shell does not have to parse Nostr event JSON in the view layer
/// (aim.md §6.9, Chirp thin-shell rule).
pub(super) fn parse_repost_inner(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return (None, None);
    }
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let inner_id = value.get("id").and_then(|v| v.as_str()).map(str::to_owned);
    let inner_content = value
        .get("content")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    (inner_id, inner_content)
}

/// `true` when `s` is exactly 64 lowercase hex characters — the canonical
/// form of a Nostr event id. Used by `lookup_for_primary_id` to choose
/// between a direct `events.get` lookup (event-id-form `primary_id`) and
/// the coordinate scan (`kind:pubkey:d_tag` form). Coordinate-form
/// strings never match (kind digits <= 5 chars, then `:`, then a 64-hex
/// pubkey, etc. — total length differs from 64 in every legal case).
pub(super) fn is_hex64_lower(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn hex64_to_bytes32(s: &str) -> Option<[u8; 32]> {
    if !is_hex64_lower(s) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = nibble(s.as_bytes()[i * 2])?;
        let lo = nibble(s.as_bytes()[i * 2 + 1])?;
        *byte = (hi << 4) | lo;
    }
    Some(out)
}

#[inline]
fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

pub(super) fn nmp_store_to_kernel_stored(e: nmp_store::StoredEvent) -> StoredEvent {
    StoredEvent {
        id: e.raw.id.clone(),
        author: e.raw.pubkey.clone(),
        kind: e.raw.kind,
        created_at: e.raw.created_at,
        tags: e.raw.tags.clone(),
        content: e.raw.content.clone(),
        relay_count: 1,
    }
}

/// Pluralized affordance label for the "Show N earlier" header above the
/// focused thread item. Empty when `count == 0` so the host renders nothing
/// without a branch (host renders `Text(label)` unconditionally; an empty
/// string collapses to a no-op). Plain English form — see aim.md §6
/// anti-pattern #1: native must not duplicate pluralization.
pub(super) fn format_previous_count_label(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "Show 1 earlier note".to_string(),
        n => format!("Show {n} earlier notes"),
    }
}

/// Pluralized affordance label for the "N more replies" footer below the
/// focused thread item. Empty when `count == 0`. Same rationale as
/// [`format_previous_count_label`].
pub(super) fn format_next_count_label(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "1 more reply".to_string(),
        n => format!("{n} more replies"),
    }
}

#[cfg(test)]
mod repost_inner_tests {
    use super::parse_repost_inner;

    #[test]
    fn empty_content_returns_none() {
        assert_eq!(parse_repost_inner(""), (None, None));
    }

    #[test]
    fn non_object_content_returns_none() {
        // NIP-18 reposts MAY ship empty `content`; Twitter-style "RT @..." plain
        // text is non-protocol but seen in the wild — both fall back cleanly.
        assert_eq!(parse_repost_inner("RT some text"), (None, None));
        assert_eq!(parse_repost_inner("[1, 2, 3]"), (None, None));
        assert_eq!(parse_repost_inner("   "), (None, None));
    }

    #[test]
    fn malformed_json_returns_none() {
        assert_eq!(parse_repost_inner("{not json"), (None, None));
        assert_eq!(parse_repost_inner("{\"id\":}"), (None, None));
    }

    #[test]
    fn well_formed_inner_event_extracts_id_and_content() {
        let raw = r#"{"id":"abc123","pubkey":"def","kind":1,"content":"hello world","tags":[]}"#;
        let (id, content) = parse_repost_inner(raw);
        assert_eq!(id.as_deref(), Some("abc123"));
        assert_eq!(content.as_deref(), Some("hello world"));
    }

    #[test]
    fn partial_inner_event_only_extracts_present_fields() {
        let (id, content) = parse_repost_inner(r#"{"id":"abc","kind":1}"#);
        assert_eq!(id.as_deref(), Some("abc"));
        assert_eq!(content, None);

        let (id, content) = parse_repost_inner(r#"{"content":"hi"}"#);
        assert_eq!(id, None);
        assert_eq!(content.as_deref(), Some("hi"));
    }

    #[test]
    fn non_string_id_or_content_falls_back_to_none() {
        // A relay sending a numeric `id` field is malformed per NIP-01; the
        // extractor must not panic and must not coerce — we degrade silently.
        let (id, content) = parse_repost_inner(r#"{"id":42,"content":null}"#);
        assert_eq!(id, None);
        assert_eq!(content, None);
    }

    #[test]
    fn leading_whitespace_is_tolerated() {
        let raw = "  \n  {\"id\":\"x\",\"content\":\"y\"}";
        let (id, content) = parse_repost_inner(raw);
        assert_eq!(id.as_deref(), Some("x"));
        assert_eq!(content.as_deref(), Some("y"));
    }
}

#[cfg(test)]
mod thread_label_tests {
    use super::{format_next_count_label, format_previous_count_label};

    #[test]
    fn previous_count_label_pluralizes_correctly() {
        assert_eq!(format_previous_count_label(0), "");
        assert_eq!(format_previous_count_label(1), "Show 1 earlier note");
        assert_eq!(format_previous_count_label(2), "Show 2 earlier notes");
        assert_eq!(format_previous_count_label(42), "Show 42 earlier notes");
    }

    #[test]
    fn next_count_label_pluralizes_correctly() {
        assert_eq!(format_next_count_label(0), "");
        assert_eq!(format_next_count_label(1), "1 more reply");
        assert_eq!(format_next_count_label(2), "2 more replies");
        assert_eq!(format_next_count_label(99), "99 more replies");
    }
}
