//! `claimed_event_embeds_json` sidecar — issue #1767 (web twin of #1283).
//!
//! The web wasm runtime emits a kernel-authored FlatBuffers update frame whose
//! `claimed_events` (KCEV) typed projection carries **raw** protocol data
//! (kind, tags, content) with no kind-dependent branching — the kernel performs
//! no `match kind` dispatch (D0: that is a *rendering* concern owned by
//! `nmp-content`). Before this change the web shell re-implemented the
//! NIP-23 / NIP-84 tag parsing in TypeScript (`NostrKindRegistry`'s
//! `tag(model, "title" | "image" | …)` columns), re-deriving protocol policy
//! the kernel already owns — exactly the divergence #1283 fixed on iOS.
//!
//! This module is the web composition root's responsibility, mirroring the
//! native `nmp-ffi::embed_sidecar`:
//!
//! 1. A post-encode frame observer (installed via
//!    [`nmp_wasm::WasmRuntime::install_frame_observer`]) decodes the KCEV from
//!    each frame's bytes, calls [`nmp_content::resolve_embed_projection`] on
//!    every row, and stores the resolved `primary_id -> EmbeddedEventEnvelope`
//!    map in a shared slot ([`EmbedSidecarSlot`]).
//! 2. A typed snapshot projection registered on the reducer reads that slot on
//!    every subsequent tick and contributes a `claimed_event_embeds_json` row
//!    whose payload is the `serde_json` encoding of the map — the `nmp-content`
//!    resolver output / `EmbeddedEventEnvelope` serde shape (camelCase fields,
//!    `EmbedKindProjection` as `{ "variant": …, "data": … }`). The web shell
//!    `JSON.parse`s the payload and renders the article/highlight/quote cards
//!    from the pre-resolved `projection` instead of re-parsing raw tags. (This
//!    JSON is consumed by the web TS only; iOS decodes the native
//!    `claimed_event_embeds` NEMB FlatBuffer — a different wire format that
//!    shares this resolution logic but not this serde JSON.)
//!
//! ## Option A (JSON), not Option B (NEMB FlatBuffer)
//!
//! The lead chose JSON parity with iOS over a typed `NEMB` FlatBuffer sidecar
//! for the web (#1767). We therefore emit `serde_json` bytes here rather than
//! `nmp_content::wire::encode_claimed_event_embeds`, and use the distinct key
//! `claimed_event_embeds_json` so it is never confused with the native typed
//! `claimed_event_embeds` (NEMB) projection — the two share resolution logic
//! (`nmp_content`) but not wire format. JSON-on-the-wasm-wire is an established
//! seam (e.g. `recent_routing_decisions_json`).
//!
//! ## One-tick lag
//!
//! The slot is populated from frame N (in the observer, after encode) and read
//! on frame N+1 (by the projection closure, during the next encode). This is
//! identical to the native sidecar's one-tick lag and acceptable: the
//! claimed-events flow is already async (the kernel fetches the event on demand
//! then surfaces it on the next snapshot), so one extra push-cycle is invisible.
//!
//! ## D0 / D6
//!
//! - D0: kind dispatch lives in `nmp-content`; this crate is the composition
//!   root (the one layer legally above both `nmp-core` and `nmp-content`),
//!   exactly like `nmp-ffi` on native.
//! - D6: every failure path (missing KCEV entry, decode error, poisoned mutex,
//!   serialise error) degrades to an empty map / omitted bytes — never a panic.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use nmp_content::{
    resolve_embed_projection, EmbedKindProjection, EmbeddedEventEnvelope, RenderContext,
    RenderContextWire,
};
use nmp_core::substrate::KernelEvent;
use nmp_core::typed_projections::{
    decode_claimed_events, ClaimedEventRow, CLAIMED_EVENTS_SCHEMA_ID,
};
use nmp_core::{decode_snapshot_typed_projections, TypedProjectionData};
use nmp_wasm::WasmRuntime;

/// Snapshot-projection key for the JSON embed sidecar (issue #1767). Distinct
/// from the native typed `claimed_event_embeds` (NEMB) key so the two wire
/// forms can never be confused on a shared decoder.
pub(crate) const EMBED_SIDECAR_JSON_KEY: &str = "claimed_event_embeds_json";

/// Schema version for the JSON embed sidecar. Bump on any breaking change to
/// the serialised `EmbeddedEventEnvelope` shape (which must stay in lockstep
/// with the web TS `EmbeddedEventModel` mirror that decodes this JSON).
const EMBED_SIDECAR_JSON_SCHEMA_VERSION: u32 = 1;

/// Shared slot carrying the latest resolved embed map
/// (`primary_id -> EmbeddedEventEnvelope`). The frame observer writes it; the
/// typed-projection closure reads it. `Arc<Mutex<…>>` (not `Rc<RefCell<…>>`)
/// because `register_typed_snapshot_projection` requires the closure to be
/// `Send + Sync` — even though, on wasm32, it is only ever touched from the
/// single JS-event-loop thread. `None` before the first frame; `Some(map)`
/// (possibly empty) after.
pub(crate) type EmbedSidecarSlot = Arc<Mutex<Option<BTreeMap<String, EmbeddedEventEnvelope>>>>;

/// Convert a decoded [`ClaimedEventRow`] into a [`KernelEvent`] for
/// `resolve_embed_projection`. Mirrors `nmp-ffi::embed_sidecar::row_to_kernel_event`.
fn row_to_kernel_event(row: &ClaimedEventRow) -> KernelEvent {
    KernelEvent {
        id: row.id.clone(),
        author: row.author_pubkey.clone(),
        kind: row.kind,
        created_at: row.created_at,
        tags: row.tags.clone(),
        content: row.content.clone(),
        relay_provenance: Vec::new(),
    }
}

/// Build the sidecar [`EmbeddedEventEnvelope`] around a resolved projection.
/// Mirrors the native sidecar's envelope shape exactly: `uri=""`, `depth=0`,
/// `max_depth=4`, `collapsed=false`, `collapse_reason=None`.
fn build_envelope(primary_id: &str, projection: EmbedKindProjection) -> EmbeddedEventEnvelope {
    EmbeddedEventEnvelope {
        uri: String::new(),
        primary_id: primary_id.to_string(),
        render_context: RenderContextWire {
            depth: 0,
            max_depth: 4,
            visited: Vec::new(),
        },
        projection,
        collapsed: false,
        collapse_reason: None,
    }
}

/// Resolve every `claimed_events` row in `frame_bytes` into the shared envelope
/// map and store it in `slot`. The web twin of
/// `nmp-ffi::embed_sidecar::update_embed_sidecar_from_frame`. Silent no-op on
/// any decode failure (D6); leaves the previous map intact when the frame has
/// no KCEV entry (stable, no flicker).
pub(crate) fn update_embed_sidecar_from_frame(frame_bytes: &[u8], slot: &EmbedSidecarSlot) {
    let Ok(projections) = decode_snapshot_typed_projections(frame_bytes) else {
        return;
    };
    let Some(kcev_entry) = projections
        .iter()
        .find(|e| e.schema_id == CLAIMED_EVENTS_SCHEMA_ID)
    else {
        // No claimed_events this frame — keep the slot as-is.
        return;
    };
    let Ok(model) = decode_claimed_events(&kcev_entry.payload) else {
        return;
    };

    if model.entries.is_empty() {
        // Explicit empty map — clear the slot so the projection sees {} not stale.
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(BTreeMap::new());
        }
        return;
    }

    let ctx = RenderContext::new();
    let mut map: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    for (primary_id, row) in &model.entries {
        let event = row_to_kernel_event(row);
        let projection: EmbedKindProjection = resolve_embed_projection(&event, &ctx);
        map.insert(primary_id.clone(), build_envelope(primary_id, projection));
    }

    if let Ok(mut guard) = slot.lock() {
        *guard = Some(map);
    }
}

/// Read the current resolved embed map from the slot, cloning under the lock.
/// Empty map when the slot is `None` (first tick) or poisoned (D6).
fn snapshot_map(slot: &EmbedSidecarSlot) -> BTreeMap<String, EmbeddedEventEnvelope> {
    slot.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Build the JSON `TypedProjectionData` for the embed sidecar from `slot`.
///
/// Always returns a present row (D1) — an empty map serialises to `{}`. The
/// payload is `serde_json` bytes of `BTreeMap<String, EmbeddedEventEnvelope>`,
/// the `nmp-content` resolver output / `EmbeddedEventEnvelope` serde shape the
/// web TS decodes (NOT the shape iOS decodes — iOS reads the native
/// `claimed_event_embeds` NEMB FlatBuffer). `file_identifier` is empty (JSON
/// has no FlatBuffers file id). D6: a serialise hiccup degrades to `{}`.
pub(crate) fn read_embed_sidecar_json(slot: &EmbedSidecarSlot) -> TypedProjectionData {
    let map = snapshot_map(slot);
    let payload = serde_json::to_vec(&map).unwrap_or_else(|_| b"{}".to_vec());
    TypedProjectionData {
        key: EMBED_SIDECAR_JSON_KEY.to_string(),
        schema_id: EMBED_SIDECAR_JSON_KEY.to_string(),
        schema_version: EMBED_SIDECAR_JSON_SCHEMA_VERSION,
        file_identifier: String::new(),
        payload,
        ..Default::default()
    }
}

/// Wire the JSON embed sidecar into `runtime`: create the slot, install the
/// post-encode frame observer that populates it, and register the
/// `claimed_event_embeds_json` typed snapshot projection that reads it.
///
/// Call once after `WasmRuntime::new()`, before `Start`. Mirrors
/// `nmp-ffi::embed_sidecar::install_embed_sidecar_projection`.
pub(crate) fn setup_embed_sidecar(runtime: &WasmRuntime) {
    let slot: EmbedSidecarSlot = Arc::new(Mutex::new(None));

    // Frame observer (populator) — fires AFTER each frame is encoded, decodes
    // the KCEV and resolves embeds into the slot. `Rc<dyn Fn(&[u8])>` is the
    // wasm-correct shape (single-threaded JS event loop).
    let observer_slot = Arc::clone(&slot);
    let observer: Rc<dyn Fn(&[u8])> = Rc::new(move |bytes: &[u8]| {
        update_embed_sidecar_from_frame(bytes, &observer_slot);
    });
    runtime.install_frame_observer(observer);

    // Typed snapshot projection (reader) — reads the slot on every tick and
    // emits the JSON sidecar row.
    let projection_slot = Arc::clone(&slot);
    runtime
        .reducer_handle()
        .borrow()
        .register_typed_snapshot_projection(EMBED_SIDECAR_JSON_KEY, move || {
            Some(read_embed_sidecar_json(&projection_slot))
        });
}

#[cfg(test)]
#[path = "embed_sidecar_tests.rs"]
mod tests;
