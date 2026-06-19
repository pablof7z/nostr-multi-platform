use super::*;
use nmp_core::store::{RawEvent, StoredEvent};
use nmp_core::substrate::KernelEvent;
use std::sync::Arc;

fn stored(kind: u32, tags: Vec<Vec<&str>>, content: &str) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: "e".repeat(64),
            pubkey: "a".repeat(64),
            created_at: 1_700_000_000,
            kind,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: content.to_string(),
            sig: "f".repeat(128),
        }),
        received_at_ms: 0,
    }
}

#[test]
fn rejects_non_picture_kind_and_picture_without_images() {
    assert!(try_from_event(&stored(1, vec![], "")).is_none());
    assert!(try_from_event(&stored(KIND_PICTURE_EVENT, vec![], "")).is_none());
}

#[test]
fn decodes_picture_event_fields() {
    let event = stored(
        KIND_PICTURE_EVENT,
        vec![
            vec!["title", "Costa Rica"],
            vec![
                "imeta",
                "url https://cdn.example/a.jpg",
                "m image/jpeg",
                "x abc",
                "dim 3024x4032",
                "alt Coast",
            ],
            vec!["content-warning", "nudity"],
            vec!["p", "pubkey1"],
            vec!["t", "travel"],
            vec!["m", "image/jpeg"],
            vec!["x", "abc"],
            vec!["location", "Costa Rica"],
            vec!["g", "9q5c"],
            vec!["L", "ISO-639-1"],
            vec!["l", "en", "ISO-639-1"],
        ],
        "caption",
    );

    let record = try_from_event(&event).unwrap();
    assert_eq!(record.title.as_deref(), Some("Costa Rica"));
    assert_eq!(record.content, "caption");
    assert_eq!(record.images.len(), 1);
    assert_eq!(record.images[0].url, "https://cdn.example/a.jpg");
    assert_eq!(record.tagged_pubkeys, vec!["pubkey1"]);
    assert_eq!(record.hashtags, vec!["travel"]);
    assert_eq!(record.media_types, vec!["image/jpeg"]);
    assert_eq!(record.hashes, vec!["abc"]);
    assert_eq!(record.location.as_deref(), Some("Costa Rica"));
    assert_eq!(record.geohash.as_deref(), Some("9q5c"));
    assert_eq!(
        record.language_tags,
        vec![
            vec!["L".to_string(), "ISO-639-1".to_string()],
            vec!["l".to_string(), "en".to_string(), "ISO-639-1".to_string()],
        ]
    );
}

#[test]
fn kernel_event_path_matches_stored_event_path() {
    let event = KernelEvent {
        id: "id".into(),
        author: "author".into(),
        kind: KIND_PICTURE_EVENT,
        created_at: 42,
        tags: vec![vec![
            "imeta".into(),
            "url https://cdn.example/a.png".into(),
            "m image/png".into(),
        ]],
        content: "caption".into(),
        relay_provenance: Vec::new(),
    };

    let record = try_from_kernel_event(&event).unwrap();
    assert_eq!(record.event_id, "id");
    assert_eq!(record.author, "author");
    assert_eq!(record.images[0].mime.as_deref(), Some("image/png"));
}
