//! Decoder half (read side) per `docs/design/kind-wrappers.md` §3.1.
//!
//! Pure, allocation-bounded, no I/O. Returns the typed `ArticleRecord` an app
//! views read instead of re-parsing tag arrays in hot paths (D8).
//!
//! Decoder name is the uniform `try_from_event` per PD-010 — every NMP
//! protocol crate exposes this verb so consumers can write
//! `nmp_nipNN::try_from_event(&event)` without learning per-crate vocabulary.

use nmp_core::store::StoredEvent;
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_LONG_FORM_ARTICLE;

/// Decoded NIP-23 article. Immutable; produced once at ingest, read everywhere
/// per `kind-wrappers.md` §1 (no read-side mutable wrappers; that's the D4
/// violation NDK's `article.title = "foo"` setter pattern commits).
///
/// `created_at` and `tags` are preserved alongside the design-doc surface
/// because: (a) views sort/filter by `created_at`; (b) callers occasionally
/// need to surface unknown tags (e.g. `t` topic tags) without re-fetching the
/// raw event from the store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleRecord {
    /// Hex event id (64 chars).
    pub event_id: String,
    /// Hex pubkey of the article author (64 chars).
    pub author: String,
    /// NIP-33 `d` tag value — the per-author stable article identifier.
    pub d_tag: String,
    /// First `title` tag value, if present.
    pub title: Option<String>,
    /// First `image` tag value (cover image URL), if present.
    pub image: Option<String>,
    /// First `summary` tag value, if present.
    pub summary: Option<String>,
    /// First `published_at` tag value parsed as unix seconds, if present and
    /// numeric. Distinct from `created_at` — NIP-23 articles can be republished
    /// (kind:30023 is parameterized-replaceable) while preserving the original
    /// publish time in this tag.
    pub published_at: Option<u64>,
    /// `created_at` from the event header (unix seconds). Always set.
    pub created_at: u64,
    /// Article body (markdown per NIP-23).
    pub content: String,
    /// Raw tags preserved verbatim for callers that need unknown-tag access
    /// (e.g. `t` topic tags, `r` references, custom client tags).
    pub tags: Vec<Vec<String>>,
}

/// Decode a stored event into an `ArticleRecord`.
///
/// Returns `None` when:
/// - `event.kind != 30023` (wrong kind), or
/// - the required NIP-33 `d` tag is missing.
///
/// Per the spec, `title` / `image` / `summary` / `published_at` are all
/// optional. `published_at` that fails to parse as `u64` is treated as absent
/// (the design-doc note in §9 anti-pattern #8 mandates normalisation at decode;
/// we choose strict-numeric-or-`None` over silent ms→s coercion to avoid the
/// NDK pitfall).
pub fn try_from_event(event: &StoredEvent) -> Option<ArticleRecord> {
    let raw = event.raw.as_ref();
    if raw.kind != KIND_LONG_FORM_ARTICLE {
        return None;
    }

    let d_tag = first_tag_value(&raw.tags, "d")?.to_string();

    Some(ArticleRecord {
        event_id: raw.id.clone(),
        author: raw.pubkey.clone(),
        d_tag,
        title: first_tag_value(&raw.tags, "title").map(str::to_string),
        image: first_tag_value(&raw.tags, "image").map(str::to_string),
        summary: first_tag_value(&raw.tags, "summary").map(str::to_string),
        published_at: first_tag_value(&raw.tags, "published_at")
            .and_then(|s| s.parse::<u64>().ok()),
        created_at: raw.created_at,
        content: raw.content.clone(),
        tags: raw.tags.clone(),
    })
}

/// Return the second column of the first tag whose first column equals `key`.
fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::{RawEvent, StoredEvent};
    use std::sync::Arc;

    fn make_stored(kind: u32, tags: Vec<Vec<String>>, content: &str) -> StoredEvent {
        StoredEvent {
            raw: Arc::new(RawEvent {
                id: "a".repeat(64),
                pubkey: "b".repeat(64),
                created_at: 1_700_000_000,
                kind,
                tags,
                content: content.into(),
                sig: "c".repeat(128),
            }),
            received_at_ms: 0,
        }
    }

    #[test]
    fn first_tag_value_returns_value_when_present() {
        let tags = vec![vec!["d".into(), "hello".into()]];
        assert_eq!(first_tag_value(&tags, "d"), Some("hello"));
    }

    #[test]
    fn first_tag_value_returns_none_when_key_missing() {
        let tags = vec![vec!["d".into(), "hello".into()]];
        assert_eq!(first_tag_value(&tags, "title"), None);
    }

    #[test]
    fn first_tag_value_returns_none_when_tag_is_only_key() {
        // A tag like `["e"]` with no value should not panic and should yield None.
        let tags = vec![vec!["d".into()]];
        assert_eq!(first_tag_value(&tags, "d"), None);
    }

    #[test]
    fn first_tag_value_returns_first_when_duplicated() {
        let tags = vec![
            vec!["title".into(), "first".into()],
            vec!["title".into(), "second".into()],
        ];
        assert_eq!(first_tag_value(&tags, "title"), Some("first"));
    }

    #[test]
    fn try_from_event_returns_some_for_kind_30023_with_d_tag() {
        let event = make_stored(30023, vec![vec!["d".into(), "intro".into()]], "body");
        let record = try_from_event(&event).expect("decoder accepts a minimal article");
        assert_eq!(record.d_tag, "intro");
        assert_eq!(record.content, "body");
        assert_eq!(record.title, None);
        assert_eq!(record.image, None);
        assert_eq!(record.summary, None);
        assert_eq!(record.published_at, None);
    }

    #[test]
    fn try_from_event_returns_none_when_kind_is_not_30023() {
        let event = make_stored(1, vec![vec!["d".into(), "x".into()]], "");
        assert!(try_from_event(&event).is_none());
    }

    #[test]
    fn try_from_event_returns_none_when_d_tag_missing() {
        // d tag required per NIP-33 / NIP-23 parameterized-replaceable contract.
        let event = make_stored(30023, vec![vec!["title".into(), "Untitled".into()]], "");
        assert!(try_from_event(&event).is_none());
    }

    #[test]
    fn try_from_event_extracts_published_at_when_numeric() {
        let event = make_stored(
            30023,
            vec![
                vec!["d".into(), "x".into()],
                vec!["published_at".into(), "1690000000".into()],
            ],
            "",
        );
        let record = try_from_event(&event).unwrap();
        assert_eq!(record.published_at, Some(1_690_000_000));
    }

    #[test]
    fn try_from_event_treats_non_numeric_published_at_as_absent() {
        // Better None than silently coercing — the design's anti-pattern §9 #8
        // warns against ms↔s magic. Strict numeric or None is the safe path.
        let event = make_stored(
            30023,
            vec![
                vec!["d".into(), "x".into()],
                vec!["published_at".into(), "not-a-timestamp".into()],
            ],
            "",
        );
        let record = try_from_event(&event).unwrap();
        assert_eq!(record.published_at, None);
    }

    #[test]
    fn try_from_event_preserves_tags_verbatim() {
        let event = make_stored(
            30023,
            vec![
                vec!["d".into(), "x".into()],
                vec!["t".into(), "rust".into()],
                vec!["r".into(), "https://example.com".into()],
            ],
            "",
        );
        let record = try_from_event(&event).unwrap();
        assert_eq!(record.tags.len(), 3);
        assert_eq!(record.tags[1], vec!["t".to_string(), "rust".to_string()]);
    }

    #[test]
    fn try_from_event_carries_created_at_from_header() {
        let event = make_stored(30023, vec![vec!["d".into(), "x".into()]], "");
        let record = try_from_event(&event).unwrap();
        assert_eq!(record.created_at, 1_700_000_000);
    }
}
