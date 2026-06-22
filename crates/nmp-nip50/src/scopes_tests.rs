//! Extraction + tokenization tests for the three NIP-50 public scopes (#1811).

use std::sync::Arc;

use nmp_store::{tokenize, RawEvent, StoredEvent};

use super::*;

fn stored(kind: u32, content: &str, tags: Vec<Vec<String>>) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: "00".repeat(32),
            pubkey: "01".repeat(32),
            created_at: 1,
            kind,
            tags,
            content: content.to_string(),
            sig: "a".repeat(128),
        }),
        received_at_ms: 0,
    }
}

/// Collect the tokenized text for a given field id across an extraction.
fn tokens_for(pairs: &[(SearchField, String)], field_id: u16) -> Vec<String> {
    pairs
        .iter()
        .filter(|(f, _)| f.id == field_id)
        .flat_map(|(_, text)| tokenize(text))
        .collect()
}

#[test]
fn profile_extracts_metadata_fields() {
    let scope = ProfileSearchScope::new();
    let content = r#"{"name":"satoshi","display_name":"Satoshi Nakamoto","about":"Bitcoin creator","nip05":"satoshi@example.com"}"#;
    let pairs = scope.extract(&stored(0, content, vec![]));

    // name (field 0)
    assert_eq!(tokens_for(&pairs, 0), vec!["satoshi"]);
    // display_name (field 1)
    assert_eq!(tokens_for(&pairs, 1), vec!["satoshi", "nakamoto"]);
    // nip05 (field 2) — '@' and '.' are separators
    assert_eq!(tokens_for(&pairs, 2), vec!["satoshi", "example", "com"]);
    // about (field 3)
    assert_eq!(tokens_for(&pairs, 3), vec!["bitcoin", "creator"]);
}

#[test]
fn profile_accepts_legacy_camelcase_display_name() {
    let scope = ProfileSearchScope::new();
    let content = r#"{"displayName":"Hal Finney"}"#;
    let pairs = scope.extract(&stored(0, content, vec![]));
    assert_eq!(tokens_for(&pairs, 1), vec!["hal", "finney"]);
}

#[test]
fn profile_extract_tolerates_non_object_content() {
    let scope = ProfileSearchScope::new();
    assert!(scope.extract(&stored(0, "not json", vec![])).is_empty());
    assert!(scope.extract(&stored(0, "[1,2,3]", vec![])).is_empty());
}

#[test]
fn note_extracts_content() {
    let scope = NoteSearchScope::new();
    let pairs = scope.extract(&stored(1, "Hello Nostr World", vec![]));
    assert_eq!(tokens_for(&pairs, 0), vec!["hello", "nostr", "world"]);
}

#[test]
fn note_extract_skips_empty_content() {
    let scope = NoteSearchScope::new();
    assert!(scope.extract(&stored(1, "", vec![])).is_empty());
}

#[test]
fn longform_extracts_title_summary_body() {
    let scope = LongFormSearchScope::new();
    let tags = vec![
        vec!["title".into(), "My Article".into()],
        vec!["summary".into(), "A short summary".into()],
        vec!["d".into(), "slug".into()],
    ];
    let pairs = scope.extract(&stored(30_023, "The body content here", tags));

    assert_eq!(tokens_for(&pairs, 0), vec!["my", "article"]);
    assert_eq!(tokens_for(&pairs, 1), vec!["short", "summary"]);
    assert_eq!(tokens_for(&pairs, 2), vec!["the", "body", "content", "here"]);
}

#[test]
fn longform_bounds_body_prefix() {
    let scope = LongFormSearchScope::new();
    // 5000 'x' separated would be one giant token; build distinct tokens past 4KB.
    let body = "word ".repeat(2000); // ~10000 bytes
    let pairs = scope.extract(&stored(30_023, &body, vec![]));
    let body_text: String = pairs
        .iter()
        .find(|(f, _)| f.id == 2)
        .map(|(_, t)| t.clone())
        .unwrap();
    assert!(body_text.len() <= 4096, "body prefix must be bounded");
}

#[test]
fn longform_missing_tags_extracts_only_body() {
    let scope = LongFormSearchScope::new();
    let pairs = scope.extract(&stored(30_023, "just a body", vec![]));
    assert!(tokens_for(&pairs, 0).is_empty());
    assert!(tokens_for(&pairs, 1).is_empty());
    assert_eq!(tokens_for(&pairs, 2), vec!["just", "body"]);
}

#[test]
fn all_scopes_are_public_and_cache_both() {
    for spec in [
        ProfileSearchScope::new().spec(),
        NoteSearchScope::new().spec(),
        LongFormSearchScope::new().spec(),
    ] {
        assert_eq!(spec.privacy, nmp_core::substrate::SearchPrivacyPolicy::PublicIndexable);
        assert_eq!(spec.cache_mode, nmp_core::substrate::CacheSearchMode::Both);
    }
}

#[test]
fn scope_labels_are_stable() {
    assert_eq!(
        ProfileSearchScope::new().spec().scope,
        SearchScopeId::from_label(SCOPE_LABEL_PROFILES)
    );
    assert_eq!(
        NoteSearchScope::new().spec().scope,
        SearchScopeId::from_label(SCOPE_LABEL_NOTES)
    );
    assert_eq!(
        LongFormSearchScope::new().spec().scope,
        SearchScopeId::from_label(SCOPE_LABEL_LONGFORM)
    );
}
