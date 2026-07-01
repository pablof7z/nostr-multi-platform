//! Unit tests for the `refs.event.envelopes` sidecar (resolve path + wire
//! shape). Extracted from `embed_sidecar.rs` to keep it under the 500 LOC gate.

use nmp_content::wire::{
    decode_ref_event_envelopes, EMBED_SIDECAR_PROJECTION_KEY, EMBED_SIDECAR_SCHEMA_ID,
};
use nmp_content::{
    resolve_embed_projection, EmbedKindProjection, EmbeddedEventEnvelope, RenderContext,
};
use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch, REFS_EVENT_KEY};
use nmp_core::typed_projections::{
    encode_claimed_events, ClaimedEventRow, ClaimedEventsModel, CLAIMED_EVENTS_SCHEMA_ID,
};
use nmp_core::{encode_snapshot_frame, SnapshotEnvelope, TypedProjectionData};
use std::collections::BTreeMap;

use super::{
    build_envelope, new_embed_sidecar_slot, read_embed_sidecar_typed, row_to_kernel_event,
    update_embed_sidecar_from_frame,
};

fn make_claimed_event_row(
    primary_id: &str,
    id: &str,
    author: &str,
    kind: u32,
    content: &str,
    tags: Vec<Vec<String>>,
) -> ClaimedEventRow {
    ClaimedEventRow {
        primary_id: primary_id.to_string(),
        id: id.to_string(),
        author_pubkey: author.to_string(),
        kind,
        created_at: 1710000000,
        tags,
        content: content.to_string(),
        content_tree_bytes: Vec::new(),
        signed_event_json: None,
    }
}

fn refs_event_frame(primary_id: &str, row: ClaimedEventRow, rev: u64) -> Vec<u8> {
    let row_payload = encode_claimed_events(&ClaimedEventsModel {
        entries: vec![(primary_id.to_string(), row)],
    });
    encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_EVENT_KEY.to_string(),
            schema_id: REFS_EVENT_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "event".to_string(),
                baseline: rev == 1,
                rows: vec![RefRow::changed(primary_id, rev, row_payload)],
            }),
            ..Default::default()
        }],
    )
}

fn claimed_events_only_frame(primary_id: &str, row: ClaimedEventRow) -> Vec<u8> {
    encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_id: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_claimed_events(&ClaimedEventsModel {
                entries: vec![(primary_id.to_string(), row)],
            }),
            ..Default::default()
        }],
    )
}

fn install_nip23_embed_adapter() {
    nmp_nip23::register_content_embed_projection_adapter();
}

#[test]
fn resolve_short_note_row_produces_short_note_projection() {
    let row = make_claimed_event_row(
        "aabbcc",
        "aabbcc",
        &"aa".repeat(32),
        1,
        "Hello nostr",
        vec![],
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    assert!(
        matches!(proj, EmbedKindProjection::ShortNote(_)),
        "kind:1 must resolve to ShortNote"
    );
}

#[test]
fn resolve_article_row_produces_article_projection() {
    install_nip23_embed_adapter();
    let tags = vec![vec!["d".to_string(), "my-article".to_string()]];
    let row = make_claimed_event_row(
        "art456",
        "art456",
        &"bb".repeat(32),
        30023,
        "# My Article",
        tags,
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    assert!(
        matches!(proj, EmbedKindProjection::Article(_)),
        "kind:30023 must resolve to Article"
    );
}

#[test]
fn resolve_highlight_row_produces_highlight_projection() {
    let tags = vec![vec!["e".to_string(), "source-event-id".to_string()]];
    let row = make_claimed_event_row(
        "hl789",
        "hl789",
        &"cc".repeat(32),
        9802,
        "quoted text",
        tags,
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    assert!(
        matches!(proj, EmbedKindProjection::Highlight(_)),
        "kind:9802 must resolve to Highlight"
    );
}

#[test]
fn resolve_profile_row_produces_profile_projection() {
    let row = make_claimed_event_row(
        "aa".repeat(32).as_str(),
        "aa".repeat(32).as_str(),
        &"aa".repeat(32),
        0,
        r#"{"name":"Alice","picture":"https://example.com/pic.jpg"}"#,
        vec![],
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    assert!(
        matches!(proj, EmbedKindProjection::Profile(_)),
        "kind:0 must resolve to Profile"
    );
}

#[test]
fn resolve_unknown_kind_row_produces_unknown_projection() {
    let row = make_claimed_event_row(
        "unk001",
        "unk001",
        &"dd".repeat(32),
        30402,
        "classified ad",
        vec![],
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    assert!(
        matches!(proj, EmbedKindProjection::Unknown(_)),
        "unregistered kind must resolve to Unknown"
    );
}

#[test]
fn embed_sidecar_json_shape_matches_expected_variant_tag() {
    let row = make_claimed_event_row(
        "aabbcc",
        "aabbcc",
        &"aa".repeat(32),
        1,
        "Hello nostr",
        vec![],
    );
    let ctx = RenderContext::new();
    let event = row_to_kernel_event(&row);
    let proj = resolve_embed_projection(&event, &ctx);
    let json = serde_json::to_value(&proj).expect("EmbedKindProjection must serialize");
    assert_eq!(
        json.get("variant").and_then(|v| v.as_str()),
        Some("shortNote"),
        "ShortNote variant must serialize as `shortNote` (camelCase tag)"
    );
}

#[test]
fn typed_sidecar_is_present_and_empty_before_refs_event_arrives() {
    let slot = new_embed_sidecar_slot();
    let typed = read_embed_sidecar_typed(&slot);
    assert_eq!(typed.key, EMBED_SIDECAR_PROJECTION_KEY);
    assert_eq!(typed.schema_id, EMBED_SIDECAR_SCHEMA_ID);
    let decoded =
        decode_ref_event_envelopes(&typed.payload).expect("empty typed sidecar must decode");
    assert!(
        decoded.is_empty(),
        "empty refs.event store => empty typed map"
    );
}

#[test]
fn typed_sidecar_carries_the_expected_resolved_map() {
    // The JSON lane was deleted in PR #1525 (escape hatch #2 eliminated).
    // This test proves the typed FlatBuffers sidecar carries the correct
    // resolved entries for all three supported embed kinds.
    install_nip23_embed_adapter();
    let slot = new_embed_sidecar_slot();
    let ctx = RenderContext::new();
    let mut map: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    for (pid, kind, content) in [
        ("note", 1u32, "hi"),
        ("art", 30023u32, "# A"),
        ("unk", 30402u32, "x"),
    ] {
        let row = make_claimed_event_row(pid, pid, &"aa".repeat(32), kind, content, vec![]);
        let proj = resolve_embed_projection(&row_to_kernel_event(&row), &ctx);
        map.insert(pid.to_string(), build_envelope(pid, proj));
    }
    slot.lock().unwrap().envelopes = map;

    let typed = read_embed_sidecar_typed(&slot);
    let decoded = decode_ref_event_envelopes(&typed.payload).expect("typed sidecar must decode");

    assert_eq!(decoded.len(), 3, "three entries expected");
    for key in ["note", "art", "unk"] {
        assert!(decoded.contains_key(key), "typed missing {key}");
        assert_eq!(
            decoded[key].primary_id, key,
            "typed primary_id mismatch for {key}"
        );
    }
    assert!(matches!(
        decoded["note"].projection,
        EmbedKindProjection::ShortNote(_)
    ));
    assert!(matches!(
        decoded["art"].projection,
        EmbedKindProjection::Article(_)
    ));
    assert!(matches!(
        decoded["unk"].projection,
        EmbedKindProjection::Unknown(_)
    ));
}

#[test]
fn update_from_refs_event_populates_ref_event_envelopes_payload() {
    let slot = new_embed_sidecar_slot();
    let primary_id = "note";
    let row = make_claimed_event_row(
        primary_id,
        primary_id,
        &"aa".repeat(32),
        1,
        "hello from refs.event",
        vec![],
    );
    let frame = refs_event_frame(primary_id, row, 1);

    update_embed_sidecar_from_frame(&frame, &slot);

    let typed = read_embed_sidecar_typed(&slot);
    let decoded = decode_ref_event_envelopes(&typed.payload).expect("typed sidecar must decode");
    assert_eq!(decoded.len(), 1);
    assert!(matches!(
        decoded[primary_id].projection,
        EmbedKindProjection::ShortNote(_)
    ));
}

#[test]
fn update_ignores_raw_claimed_events_without_refs_event() {
    let slot = new_embed_sidecar_slot();
    let primary_id = "legacy";
    let row = make_claimed_event_row(
        primary_id,
        primary_id,
        &"aa".repeat(32),
        1,
        "legacy claimed_events must not be authoritative",
        vec![],
    );
    let frame = claimed_events_only_frame(primary_id, row);

    update_embed_sidecar_from_frame(&frame, &slot);

    let typed = read_embed_sidecar_typed(&slot);
    let decoded = decode_ref_event_envelopes(&typed.payload).expect("typed sidecar must decode");
    assert!(
        decoded.is_empty(),
        "refs.event.envelopes output must derive from refs.event, not raw claimed_events"
    );
}
