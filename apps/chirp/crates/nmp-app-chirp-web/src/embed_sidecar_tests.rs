//! Tests for the `claimed_event_embeds_json` sidecar (#1767).
//!
//! These prove the WEB twin of nmp-ffi's embed sidecar: a `claimed_events` row
//! of kind:30023 / kind:9802, once resolved, yields a JSON sidecar entry whose
//! `projection` is the kind-dispatched `EmbedKindProjection` — NOT the raw
//! tags. The JSON shape is asserted by *key* (camelCase, `{variant,data}`) so
//! it stays in lockstep with iOS's Codable mirror, which is the actual
//! correctness contract of Option A (JSON parity with iOS).
//!
//! ## Why the resolve path is exercised directly
//!
//! The full store+claim→`claimed_events`-in-frame pipeline is reachable only
//! through `Kernel`-private APIs (`ingest_pre_verified_event` etc.) which
//! `nmp-core` already tests (`claimed_events_carries_raw_author_pubkey…`). From
//! this downstream crate the only public `KernelReducer` test seam,
//! `project_raw_event_for_test`, runs the *post-insert* projection pipeline
//! without inserting into the store, so a claim can't resolve. Rather than
//! force a brittle full-ingest fake here, we exercise the load-bearing half —
//! `resolve_embed_projection` → `build_envelope` → `read_embed_sidecar_json` —
//! exactly as nmp-ffi's `typed_sidecar_carries_the_expected_resolved_map` does.
//! `update_embed_sidecar_from_frame`'s decode path is line-for-line nmp-ffi's
//! (against the same `decode_snapshot_typed_projections`/`decode_claimed_events`)
//! and is covered by `frame_with_no_claimed_events_clears_the_map` below.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_content::{resolve_embed_projection, EmbeddedEventEnvelope, RenderContext};
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelReducer;

use super::{
    build_envelope, read_embed_sidecar_json, update_embed_sidecar_from_frame, EmbedSidecarSlot,
    EMBED_SIDECAR_JSON_KEY,
};

const AUTHOR_HEX: &str = "abababababababababababababababababababababababababababababababab";

fn hex64(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

fn kernel_event(id: &str, kind: u32, content: &str, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: AUTHOR_HEX.to_string(),
        kind,
        created_at: 1_700_000_000,
        tags,
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn new_slot() -> EmbedSidecarSlot {
    Arc::new(Mutex::new(None))
}

/// Resolve `events` into the slot via the SAME path the live frame observer
/// uses (`resolve_embed_projection` + `build_envelope`), keyed by primary id.
fn seed_resolved(slot: &EmbedSidecarSlot, events: &[(String, KernelEvent)]) {
    let ctx = RenderContext::new();
    let mut map: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    for (primary_id, event) in events {
        let projection = resolve_embed_projection(event, &ctx);
        map.insert(primary_id.clone(), build_envelope(primary_id, projection));
    }
    *slot.lock().unwrap() = Some(map);
}

/// Decode the JSON sidecar payload the projection closure produces from `slot`.
fn decode_json_sidecar(slot: &EmbedSidecarSlot) -> serde_json::Value {
    let typed = read_embed_sidecar_json(slot);
    assert_eq!(typed.key, EMBED_SIDECAR_JSON_KEY);
    assert_eq!(typed.schema_id, EMBED_SIDECAR_JSON_KEY);
    assert!(
        typed.file_identifier.is_empty(),
        "JSON sidecar must carry no FlatBuffers file identifier"
    );
    serde_json::from_slice(&typed.payload).expect("sidecar payload must be valid JSON")
}

#[test]
fn article_row_yields_resolved_article_projection_json() {
    let id = hex64("3");
    let event = kernel_event(
        &id,
        30023,
        "# Body markdown",
        vec![
            vec!["d".to_string(), "my-article".to_string()],
            vec!["title".to_string(), "My Great Article".to_string()],
            vec!["summary".to_string(), "A short summary".to_string()],
            vec![
                "image".to_string(),
                "https://example.com/hero.png".to_string(),
            ],
        ],
    );

    let slot = new_slot();
    seed_resolved(&slot, &[(id.clone(), event)]);
    let json = decode_json_sidecar(&slot);

    let entry = json
        .get(&id)
        .unwrap_or_else(|| panic!("sidecar must carry primary_id `{id}`; got {json}"));

    // Envelope shape (camelCase — same as iOS Codable).
    assert_eq!(entry["primaryId"], id, "envelope.primaryId must be the key");
    assert_eq!(entry["collapsed"], false);

    // The kind-dispatched projection: `{ "variant": "article", "data": {…} }`.
    // These keys exist ONLY because `resolve_embed_projection` ran — a raw row
    // would serialize tags as `[["title", …]]`, never `data.heroImageUrl`.
    let projection = &entry["projection"];
    assert_eq!(
        projection["variant"], "article",
        "kind:30023 must resolve to the Article variant (proves the kernel-owned \
         kind dispatch ran; raw tags carry no `variant`)"
    );
    let data = &projection["data"];
    assert_eq!(data["title"], "My Great Article");
    assert_eq!(data["summary"], "A short summary");
    assert_eq!(data["heroImageUrl"], "https://example.com/hero.png");
    assert_eq!(data["dTag"], "my-article");
}

#[test]
fn highlight_row_yields_resolved_highlight_projection_json() {
    let id = hex64("4");
    let source_id = hex64("5");
    let event = kernel_event(
        &id,
        9802,
        "the highlighted text",
        vec![
            vec!["e".to_string(), source_id.clone()],
            vec!["context".to_string(), "surrounding context".to_string()],
            vec!["r".to_string(), "https://example.com/src".to_string()],
        ],
    );

    let slot = new_slot();
    seed_resolved(&slot, &[(id.clone(), event)]);
    let json = decode_json_sidecar(&slot);

    let entry = json
        .get(&id)
        .unwrap_or_else(|| panic!("sidecar must carry primary_id `{id}`; got {json}"));
    let projection = &entry["projection"];
    assert_eq!(
        projection["variant"], "highlight",
        "kind:9802 must resolve to the Highlight variant"
    );
    let data = &projection["data"];
    assert_eq!(data["highlightedText"], "the highlighted text");
    assert_eq!(data["context"], "surrounding context");
    assert_eq!(data["sourceUrl"], "https://example.com/src");
    assert_eq!(data["sourceEventId"], source_id);
}

#[test]
fn short_note_row_yields_resolved_short_note_projection_json() {
    // The quote card renders ShortNote via the resolved projection, so lock the
    // camelCase keys the web TS reads (authorDisplayName / authorPictureUrl /
    // createdAt / contentTree) against the serde output — the iOS-parity
    // contract for the quote-card path.
    let id = hex64("6");
    let event = kernel_event(&id, 1, "Hello nostr world", vec![]);

    let slot = new_slot();
    seed_resolved(&slot, &[(id.clone(), event)]);
    let json = decode_json_sidecar(&slot);

    let projection = &json[&id]["projection"];
    assert_eq!(
        projection["variant"], "shortNote",
        "kind:1 must resolve to ShortNote"
    );
    let data = &projection["data"];
    // These keys must exist (even if null) for the web shortNote accessors.
    assert!(
        data.get("createdAt").is_some(),
        "shortNote.createdAt key must be present"
    );
    assert!(
        data.get("authorDisplayName").is_some(),
        "shortNote.authorDisplayName key must be present (camelCase, web reads it)"
    );
    assert!(
        data.get("authorPictureUrl").is_some(),
        "shortNote.authorPictureUrl key must be present (camelCase, web reads it)"
    );
    // The content tree the web preview walker flattens for the quote body.
    let nodes = data["contentTree"]["nodes"]
        .as_array()
        .expect("shortNote.contentTree.nodes must be a JSON array");
    let has_text = nodes.iter().any(|n| {
        n.get("kind").and_then(|k| k.as_str()) == Some("text")
            && n.get("text").and_then(|t| t.as_str()) == Some("Hello nostr world")
    });
    assert!(
        has_text,
        "content tree must carry the note text as a `text` node; got {data}"
    );
}

#[test]
fn absent_slot_serialises_to_empty_json_object() {
    // D1: the projection is always present; an unpopulated slot is `{}`.
    let slot = new_slot();
    let json = decode_json_sidecar(&slot);
    assert!(
        json.as_object().is_some_and(|m| m.is_empty()),
        "absent slot must serialise to an empty JSON object, got {json}"
    );
}

#[test]
fn frame_with_no_claimed_events_clears_the_map() {
    // Exercises `update_embed_sidecar_from_frame`'s real decode path against a
    // genuine kernel frame. A fresh reducer's frame carries an EMPTY
    // claimed_events entry, which the populator treats as an explicit clear.
    let slot = new_slot();
    // Seed a non-empty map first so "cleared to {}" is observable.
    seed_resolved(
        &slot,
        &[(hex64("9"), kernel_event(&hex64("9"), 1, "hi", vec![]))],
    );
    assert!(
        !decode_json_sidecar(&slot).as_object().unwrap().is_empty(),
        "precondition: slot starts non-empty"
    );

    let empty_frame = KernelReducer::new().make_update_frame(true);
    update_embed_sidecar_from_frame(&empty_frame, &slot);

    let json = decode_json_sidecar(&slot);
    assert!(
        json.as_object().is_some_and(|m| m.is_empty()),
        "an empty claimed_events frame clears the slot to {{}}, got {json}"
    );
}
