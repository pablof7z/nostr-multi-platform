//! Wire-shape pinning tests. These assertions are the contract Swift / desktop
//! / broker decoders rely on. A failure here means a renaming somewhere in this
//! crate (`#[serde(rename_all = ...)]`, variant renames, struct-field renames)
//! silently broke every cross-platform shell decoder. Update host decoders
//! BEFORE relaxing any of these assertions.

use crate::mode::RenderMode;
use crate::segment::MediaKind;
use crate::wire::{ContentTreeWire, WireNode};

/// Serialize one `WireNode::Media` (wrapped in a `ContentTreeWire`) and return
/// the JSON. Tiny harness so each kind-variant test is one assertion.
fn media_node_json(kind: MediaKind) -> String {
    let tree = ContentTreeWire {
        nodes: vec![WireNode::Media {
            urls: vec!["https://example.test/a.bin".to_string()],
            media_kind: kind,
        }],
        roots: vec![0],
        mode: RenderMode::Plain,
    };
    serde_json::to_string(&tree).expect("ContentTreeWire serializes")
}

#[test]
fn media_image_wire_shape_is_pinned() {
    let json = media_node_json(MediaKind::Image);
    assert!(
        json.contains("\"kind\":\"media\""),
        "expected snake_case `kind` discriminator `media`, got: {json}"
    );
    assert!(
        json.contains("\"media_kind\":\"Image\""),
        "expected PascalCase `MediaKind::Image` payload (no `rename_all` on MediaKind), got: {json}"
    );
}

#[test]
fn media_video_wire_shape_is_pinned() {
    let json = media_node_json(MediaKind::Video);
    assert!(
        json.contains("\"kind\":\"media\""),
        "expected snake_case `kind` discriminator `media`, got: {json}"
    );
    assert!(
        json.contains("\"media_kind\":\"Video\""),
        "expected PascalCase `MediaKind::Video` payload, got: {json}"
    );
}

#[test]
fn media_audio_wire_shape_is_pinned() {
    let json = media_node_json(MediaKind::Audio);
    assert!(
        json.contains("\"kind\":\"media\""),
        "expected snake_case `kind` discriminator `media`, got: {json}"
    );
    assert!(
        json.contains("\"media_kind\":\"Audio\""),
        "expected PascalCase `MediaKind::Audio` payload, got: {json}"
    );
}

#[test]
fn media_wire_shape_round_trips_all_kinds() {
    for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
        let json = media_node_json(kind);
        let back: ContentTreeWire =
            serde_json::from_str(&json).expect("ContentTreeWire round-trips");
        assert_eq!(
            back.nodes.first(),
            Some(&WireNode::Media {
                urls: vec!["https://example.test/a.bin".to_string()],
                media_kind: kind,
            }),
            "round-trip lost the MediaKind variant"
        );
    }
}
