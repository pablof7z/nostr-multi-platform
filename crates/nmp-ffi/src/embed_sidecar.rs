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
//!    and store the pre-resolved `primary_id -> EmbeddedEventEnvelope` map in a
//!    shared slot (the single source of truth — see [`EmbedSidecarSlot`]).
//! 2. TWO snapshot projection closures registered at app-init read from that one
//!    slot on every subsequent tick and contribute `claimed_event_embeds`:
//!    - a **JSON** `Value` projection (issue #1283 transitional) for the gallery
//!      shell, which still decodes the JSON sidecar; and
//!    - a **typed** `NEMB` FlatBuffer projection
//!      ([`nmp_content::wire::encode_claimed_event_embeds`]) for the typed-frame
//!      shells (Chirp iOS + chirp-desktop), which have no JSON `payload` and so
//!      decode the typed sidecar.
//!
//! Both encoders consume the identical resolved map, so the two sidecars carry
//! the same data — a host's `typed<K> ?? json<k>` fallback lines up. Typed-frame
//! shells decode the typed key instead of duplicating the `match kind` resolver
//! in Swift/Kotlin: the iOS `EmbedHost.resolve()` / `parseProfileMetadata` /
//! `extractTopLevelMedia` methods are deleted (closing the EmbedHost D0
//! violation #1283, and fixing the #1299 inverted display_name precedence by
//! making the Rust resolver authoritative).
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
//! - D8: the listener-thread processing (decode + resolve) is pure in-process
//!   Rust — no I/O, no blocking.  Each projection closure is a cheap
//!   `Mutex::lock` read + encode — non-blocking on the actor thread (D8:
//!   projection closures must be non-blocking).
//! - D6: all failure paths (missing KCEV entry, decode error) degrade to an
//!   empty map, which the JSON closure maps to `{}` and the typed closure to a
//!   well-formed empty `NEMB` buffer — never a panic.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_content::wire::{encode_claimed_event_embeds, EMBED_SIDECAR_SCHEMA_VERSION};
use nmp_content::{
    resolve_embed_projection, EmbedKindProjection, EmbeddedEventEnvelope, RenderContext,
    RenderContextWire,
};
use nmp_core::{
    decode_snapshot_typed_projections,
    substrate::KernelEvent,
    typed_projections::{decode_claimed_events, ClaimedEventRow, CLAIMED_EVENTS_SCHEMA_ID},
    TypedProjectionData,
};
/// Snapshot-projection key for the typed FlatBuffer embed sidecar.
const EMBED_SIDECAR_KEY: &str = "claimed_event_embeds";

/// Shared slot that carries the latest resolved embed map
/// (`primary_id -> EmbeddedEventEnvelope`). This is the SINGLE source of truth
/// the listener thread writes; both the JSON projection (gallery shell) and the
/// typed FlatBuffer projection (Chirp typed-frame shell) are derived from it on
/// the actor thread. `None` before the first frame arrives; `Some(map)` (which
/// may be empty) after.
pub(crate) type EmbedSidecarSlot = Arc<Mutex<Option<BTreeMap<String, EmbeddedEventEnvelope>>>>;

/// Construct a new, empty [`EmbedSidecarSlot`].
pub(crate) fn new_embed_sidecar_slot() -> EmbedSidecarSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh slot plus a listener-thread clone in one call, so the
/// `nmp_app_new` wiring stays a single line (the listener thread takes the
/// returned `.1` by move; the `.0` is handed to
/// [`install_embed_sidecar_projection`]). Keeps the already-over-cap `lib.rs`
/// from growing (AGENTS.md file-size anti-cheat).
pub(crate) fn new_embed_sidecar_pair() -> (EmbedSidecarSlot, EmbedSidecarSlot) {
    let slot = new_embed_sidecar_slot();
    let listener = Arc::clone(&slot);
    (slot, listener)
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
        relay_provenance: Vec::new(),
    }
}

/// Build the FFI-sidecar [`EmbeddedEventEnvelope`] around a resolved projection.
///
/// Mirrors the Phase 0 JSON envelope shape: `uri=""`, `depth=0`, `max_depth=4`,
/// `collapsed=false`, `collapse_reason=None`. Both the JSON and typed encoders
/// consume this shared shape so the two sidecars carry identical data.
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

/// Called from the listener thread after every update frame.
///
/// Decodes the KCEV (`claimed_events`) typed sidecar from `frame_bytes`,
/// resolves each row's `EmbedKindProjection` via `nmp-content`, and stores the
/// resolved `primary_id -> EmbeddedEventEnvelope` map in `slot`. The next tick's
/// projection closures (JSON + typed) read from the slot and each encode their
/// own wire form (one-tick lag — see module doc).
///
/// Silent no-ops on any decode failure (D6).
pub(crate) fn update_embed_sidecar_from_frame(frame_bytes: &[u8], slot: &EmbedSidecarSlot) {
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
        // Explicit empty map — clear the slot so both sidecars see {} not stale.
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(BTreeMap::new());
        }
        return;
    }

    // Resolve each entry into the shared envelope shape.
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

/// Read the current resolved embed map from the slot, cloning it out under the
/// lock. Returns an empty map when the slot is `None` (first tick, before any
/// frame is processed) or when the mutex is poisoned (D6).
fn snapshot_map(slot: &EmbedSidecarSlot) -> BTreeMap<String, EmbeddedEventEnvelope> {
    slot.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Build the TYPED FlatBuffer (`NEMB`) form of the embed sidecar from the slot.
///
/// Always returns a present [`TypedProjectionData`] — an empty map is a
/// well-formed empty buffer (D1: present and typed even when empty). This is the
/// surface a typed-frame shell (Chirp) decodes; it carries the identical
/// resolved map as the JSON form. D6: a poisoned slot degrades to an empty
/// buffer (via [`snapshot_map`]) rather than panicking on the actor thread.
pub(crate) fn read_embed_sidecar_typed(slot: &EmbedSidecarSlot) -> TypedProjectionData {
    let map = snapshot_map(slot);
    TypedProjectionData {
        key: EMBED_SIDECAR_KEY.to_string(),
        schema_id: EMBED_SIDECAR_KEY.to_string(),
        schema_version: EMBED_SIDECAR_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(nmp_content::wire::EMBED_SIDECAR_FILE_IDENTIFIER)
            .into_owned(),
        payload: encode_claimed_event_embeds(&map),
        ..Default::default()
    }
}

/// Register BOTH the JSON and the typed `claimed_event_embeds` snapshot
/// projections on `app`, each reading the same `slot` (the resolved map the
/// listener thread writes).
///
/// * The JSON projection (`register_snapshot_projection`) feeds the gallery
///   shell, which decodes the JSON `Value` sidecar (issue #1283 transitional).
/// * The typed projection (`register_typed_snapshot_projection`) feeds the Chirp
///   typed-frame shell, which decodes the `NEMB` FlatBuffer and so needs ZERO
///   embed-resolution logic in Swift (closes the EmbedHost D0 violation #1283).
///
/// On the first tick (before the listener thread has processed any frame) the
/// slot is `None`; both closures emit empty (D1: always present). After the
/// first frame the slot holds the pre-resolved map. D8: each closure is a pure
/// `Mutex::lock` read + encode, non-blocking on the actor thread. D0: kind
/// dispatch lives in `nmp-content`; these are thin readers/encoders.
///
/// ADR-0055 R6-S2: the typed projection now uses `TypedProjectionEmissionState`
/// to omit an unchanged `claimed_event_embeds` frame when the host has declared
/// incremental-apply capability (exact byte equality, monotonic rev, freeze
/// guard on the same FrameIdentity the feed uses — one shared implementation).
///
/// Keeping the registration body here — rather than inline in `lib.rs` — keeps
/// the already-over-cap `lib.rs` from growing (AGENTS.md file-size anti-cheat).
pub(crate) fn install_embed_sidecar_projection(app: &crate::NmpApp, slot: EmbedSidecarSlot) {
    use nmp_core::projection_emission::{FrameIdentity, TypedProjectionEmissionState};
    use std::sync::atomic::Ordering;

    // R6-S2: read capability + frame-identity handles once at registration time
    // (the NmpApp APIs acquire the registry lock internally).
    let incremental_apply = app.incremental_apply_handle();
    let (frame_session_id, frame_snapshot_epoch) = app.frame_identity_handles();
    // Wrap the emission state in `Arc<Mutex<…>>` so the `Send + Sync` closure
    // requirement is satisfied. The lock is uncontested in production (only the
    // actor thread calls this closure under the registry's own mutex).
    let emission_state = Arc::new(Mutex::new(TypedProjectionEmissionState::new(
        incremental_apply,
    )));

    app.register_typed_snapshot_projection(EMBED_SIDECAR_KEY, move || {
        let typed_data = read_embed_sidecar_typed(&slot);
        // R6-S2: apply byte-equality omit (same mechanism as feed R6-S1).
        let identity = FrameIdentity {
            session_id: frame_session_id.load(Ordering::Acquire),
            snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
        };
        let Ok(mut state) = emission_state.lock() else {
            // Poisoned mutex — degrade to always-emit (D6: safe fallback).
            return Some(typed_data);
        };
        let payload = typed_data.payload.clone();
        let emit_decision = state.should_emit(payload, identity);
        drop(state);
        match emit_decision {
            None => None,
            Some((payload, projection_rev)) => Some(nmp_core::TypedProjectionData {
                payload,
                projection_rev,
                ..typed_data
            }),
        }
    });
}

// ── Unit tests ──────────────────────────────────────────────────────────────
// Extracted to sibling files to keep this file under the 500 LOC gate:
// - `embed_sidecar_tests.rs`    — resolve path + wire shape tests (pre-R6-S2).
// - `embed_sidecar_emission_tests.rs` — ADR-0055 R6-S2 cardinal-trap tests.

#[cfg(test)]
#[path = "embed_sidecar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "embed_sidecar_emission_tests.rs"]
mod emission_tests;
