//! `claimed_event_embeds` sidecar — issue #1283 / ADR-0034 §embed-sidecar.
//!
//! The kernel's `claimed_events` KCEV FlatBuffer carries raw protocol data
//! (kind, tags, content) but performs **no** kind-dependent branching — the
//! `claimed_events.fbs` schema doc (line 31-34) explicitly records that
//! invariant.  The `match event.kind` dispatch is a *rendering* concern that
//! lives in `nmp-content` (D0-clean).
//!
//! This module implements the nmp-ffi layer's responsibility:
//!
//! 1. After each update frame arrives in the listener thread, decode the KCEV
//!    typed sidecar, call `nmp_content::resolve_embed_projection` on every row,
//!    and store the pre-resolved JSON map in a shared slot.
//! 2. A JSON snapshot projection closure registered at app-init reads from
//!    that slot on every subsequent tick and contributes
//!    `projections["claimed_event_embeds"]` to the snapshot.
//!
//! Shells decode the new key instead of duplicating the `match kind` resolver
//! in Swift/Kotlin.  The iOS `EmbedHost.resolve()` / `parseProfileMetadata` /
//! `extractTopLevelMedia` methods are deleted; the gallery `EmbedHost` clone
//! is deleted identically.
//!
//! ## One-tick lag
//!
//! The embed sidecar is produced from the **previous** frame's KCEV data
//! (written in the listener thread after encode, read on the next tick's
//! projection closure).  This is acceptable: the claimed-events flow is already
//! async (kernel fetches the event on demand then surfaces it on the next
//! snapshot push), so one additional push-cycle lag is invisible to the user.
//!
//! ## D0 / D8 compliance
//!
//! - D0: kind-dispatch lives in `nmp-content`, not in the kernel.  `nmp-ffi`
//!   bridges substrate → rendering at the C-ABI boundary — the one layer that
//!   is legally above both `nmp-core` and `nmp-content`.
//! - D8: the listener-thread processing (decode + resolve + JSON serialize) is
//!   pure in-process Rust — no I/O, no blocking.  The projection closure is
//!   a cheap `Arc::clone` + `Mutex::lock` read — non-blocking on the actor
//!   thread (D8: projection closures must be non-blocking).
//! - D6: all failure paths (missing KCEV entry, decode error, serialize error)
//!   degrade to a `None`-valued slot which the projection closure maps to an
//!   empty JSON object `{}` — never a panic.

use std::sync::{Arc, Mutex};

use nmp_content::{resolve_embed_projection, EmbedKindProjection, RenderContext};
use nmp_core::{
    decode_snapshot_typed_projections,
    substrate::KernelEvent,
    typed_projections::{decode_claimed_events, ClaimedEventRow, CLAIMED_EVENTS_SCHEMA_ID},
};
use serde_json::Value;

/// Shared slot that carries the latest resolved `claimed_event_embeds` JSON
/// map (`primary_id -> EmbedKindProjection`).  `None` before the first frame
/// arrives.  Always `Some({})` or `Some({ ... })` after the first frame.
pub(crate) type EmbedSidecarSlot = Arc<Mutex<Option<Value>>>;

/// Construct a new, empty [`EmbedSidecarSlot`].
pub(crate) fn new_embed_sidecar_slot() -> EmbedSidecarSlot {
    Arc::new(Mutex::new(None))
}

/// Convert a [`ClaimedEventRow`] from the KCEV buffer into a
/// [`nmp_core::substrate::KernelEvent`] for `resolve_embed_projection`.
fn row_to_kernel_event(row: &ClaimedEventRow) -> KernelEvent {
    KernelEvent {
        id: row.id.clone(),
        author: row.author_pubkey.clone(),
        kind: row.kind,
        created_at: row.created_at,
        tags: row.tags.clone(),
        content: row.content.clone(),
    }
}

/// Called from the listener thread after every update frame.
///
/// Decodes the KCEV (`claimed_events`) typed sidecar from `frame_bytes`,
/// resolves each row's `EmbedKindProjection` via `nmp-content`, serialises
/// the result as a `primary_id -> EmbedKindProjection` JSON object, and
/// stores it in `slot`.  The next tick's projection closure reads from the
/// slot (one-tick lag — see module doc).
///
/// Silent no-ops on any decode / serialize failure (D6).
pub(crate) fn update_embed_sidecar_from_frame(
    frame_bytes: &[u8],
    slot: &EmbedSidecarSlot,
) {
    // Decode the full typed-projection sidecar from the frame.
    let Ok(projections) = decode_snapshot_typed_projections(frame_bytes) else {
        return;
    };

    // Find the KCEV entry.
    let Some(kcev_entry) = projections
        .iter()
        .find(|e| e.schema_id == CLAIMED_EVENTS_SCHEMA_ID)
    else {
        // No claimed events this frame — keep the slot as-is so the previous
        // tick's embeddings remain visible (stable, not flicker).
        return;
    };

    // Decode the FlatBuffer.
    let Ok(model) = decode_claimed_events(&kcev_entry.payload) else {
        return;
    };

    if model.entries.is_empty() {
        // Explicit empty map — clear the slot so EmbedHost sees {} not stale.
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(Value::Object(serde_json::Map::new()));
        }
        return;
    }

    // Resolve each entry.
    let ctx = RenderContext::new();
    let mut map = serde_json::Map::with_capacity(model.entries.len());
    for (primary_id, row) in &model.entries {
        let event = row_to_kernel_event(row);
        let projection: EmbedKindProjection = resolve_embed_projection(&event, &ctx);
        // Build the full envelope that the shell decodes. Mirrors
        // `EmbeddedEventEnvelope` shape so the Swift Codable decode works
        // against the existing F-CR-12 types without a new wire type.
        //
        // Keys are snake_case so the iOS `JSONDecoder.keyDecodingStrategy =
        // .convertFromSnakeCase` maps them to the camelCase Swift properties.
        // `projection` sub-object keys remain camelCase (serde `rename_all`)
        // — convertFromSnakeCase is a no-op for already-camelCase tokens.
        let envelope = serde_json::json!({
            "uri": "",
            "primary_id": primary_id,
            "depth": 0u8,
            "max_depth": 4u8,
            "collapsed": false,
            "collapse_reason": null,
            "projection": projection,
        });
        map.insert(primary_id.clone(), envelope);
    }

    if let Ok(mut guard) = slot.lock() {
        *guard = Some(Value::Object(map));
    }
}

/// Read the current embed sidecar value from the slot.
///
/// Returns `Value::Object({})` when the slot is `None` (first tick, before
/// the first frame has been processed) or when the mutex is poisoned (D6).
pub(crate) fn read_embed_sidecar(slot: &EmbedSidecarSlot) -> Value {
    slot.lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}

/// Register the `claimed_event_embeds` JSON snapshot projection on `app`.
///
/// The closure captures a clone of `app.embed_sidecar` and reads it on every
/// tick. On the first tick (before the listener thread has processed any frame)
/// the slot is `None` and the closure emits `{}` (D1: always present). After the
/// first frame the slot holds the pre-resolved map. D8: pure `Mutex::lock` read,
/// non-blocking. D0: kind dispatch lives in `nmp-content`; this is a thin reader.
///
/// Called once at app-init (see `nmp_app_new`). Keeping the registration body
/// here — rather than inline in `lib.rs` — keeps the already-over-cap `lib.rs`
/// from growing (AGENTS.md file-size anti-cheat).
pub(crate) fn install_embed_sidecar_projection(app: &crate::NmpApp) {
    let slot = Arc::clone(&app.embed_sidecar);
    app.register_snapshot_projection("claimed_event_embeds", move || read_embed_sidecar(&slot));
}

// ── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
            author_display_name: None,
            author_picture_url: None,
            kind,
            created_at: 1710000000,
            tags,
            content: content.to_string(),
        }
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
    fn read_embed_sidecar_returns_empty_object_when_slot_is_none() {
        let slot = new_embed_sidecar_slot();
        let val = read_embed_sidecar(&slot);
        assert_eq!(
            val,
            Value::Object(serde_json::Map::new()),
            "empty slot must yield an empty JSON object"
        );
    }

    #[test]
    fn embed_sidecar_json_shape_matches_expected_variant_tag() {
        // Golden test: a kind:1 row must produce a JSON envelope whose
        // `projection.variant` equals `"shortNote"` (camelCase, matching the
        // Rust `#[serde(rename_all = "camelCase")]` on the `EmbedKindProjection`
        // enum's variant discriminator — held by the EmbedKindProjection serde
        // attributes `#[serde(tag = "variant", content = "data", rename_all = "camelCase")]`).
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
}
