//! `refs.event.envelopes` derived sidecar — issue #1283 / ADR-0072
//! §embed-sidecar.
//!
//! `refs.event` is the authoritative event-reference projection. Its row
//! payloads are single-event KCEV buffers carrying raw protocol data (kind,
//! tags, content), but the row projection itself is stateful and row-grained:
//! each frame carries only changed/cleared refs and must be merged into a
//! [`nmp_core::refs::RefEventStore`]. The old whole-map `claimed_events`
//! projection is not a valid data source for embed envelopes.
//!
//! This module implements the native runtime's derived projection responsibility:
//!
//! 1. After each update frame arrives in the listener thread, apply the
//!    `refs.event` row-delta sidecar to a persistent [`RefEventStore`], resolve
//!    its current live rows through `nmp_content::resolve_embed_projection`, and
//!    store the pre-resolved `primary_id -> EmbeddedEventEnvelope` map in the
//!    same shared state.
//! 2. The snapshot projection closure registered at app-init reads from that
//!    state on every subsequent tick and contributes the derived
//!    `refs.event.envelopes` typed `NEMB` FlatBuffer projection
//!    ([`nmp_content::wire::encode_ref_event_envelopes`]) for typed-frame
//!    shells, which have no JSON `payload` and so decode the typed sidecar.
//!
//! Typed-frame shells decode the typed key instead of duplicating the
//! `match kind` resolver in Swift/Kotlin: the iOS `EmbedHost.resolve()` /
//! `parseProfileMetadata` / `extractTopLevelMedia` methods are deleted (closing
//! the EmbedHost D0 violation #1283, and fixing the #1299 inverted display_name
//! precedence by making the Rust resolver authoritative).
//!
//! ## One-tick lag
//!
//! The derived sidecar is produced from the **previous** frame's
//! `refs.event` data (written in the listener thread after encode, read on the
//! next tick's projection closure). This is acceptable: the event-ref flow is
//! already async (kernel fetches the event on demand then surfaces it on the next
//! snapshot push), so one additional push-cycle lag is invisible to the user.
//!
//! ## D0 / D8 compliance
//!
//! - D0: kind-dispatch lives in `nmp-content`, not in the kernel. The native
//!   runtime bridges substrate -> rendering for Rust hosts, while `nmp-uniffi`
//!   exposes that state through the typed binding surface (there is no
//!   separate `nmp-ffi` C-ABI crate).
//! - D8: the listener-thread processing (decode + resolve) is pure in-process
//!   Rust — no I/O, no blocking.  Each projection closure is a cheap
//!   `Mutex::lock` read + encode — non-blocking on the actor thread (D8:
//!   projection closures must be non-blocking).
//! - D6: all failure paths (missing `refs.event` entry, decode error) preserve
//!   the prior cache or degrade to a well-formed empty `NEMB` buffer on first
//!   tick — never a panic.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_content::wire::{
    encode_ref_event_envelopes, EMBED_SIDECAR_FILE_IDENTIFIER, EMBED_SIDECAR_PROJECTION_KEY,
    EMBED_SIDECAR_SCHEMA_ID, EMBED_SIDECAR_SCHEMA_VERSION,
};
use nmp_content::{
    resolve_embed_projection, EmbedKindProjection, EmbeddedEventEnvelope, RenderContext,
    RenderContextWire,
};
use nmp_core::{
    decode_snapshot_envelope, decode_snapshot_typed_projections,
    refs::{RefEventStore, REFS_EVENT_KEY},
    substrate::KernelEvent,
    typed_projections::ClaimedEventRow,
    TypedProjectionData,
};
/// Shared state for the derived `refs.event.envelopes` projection.
///
/// `ref_events` is the authoritative host-side mirror of `refs.event` row
/// deltas. `envelopes` is a render-facing cache derived from that store; it is
/// never populated from the legacy whole-map `claimed_events` projection.
#[derive(Debug, Default)]
pub(crate) struct EmbedSidecarState {
    ref_events: RefEventStore,
    envelopes: BTreeMap<String, EmbeddedEventEnvelope>,
}

/// Shared slot written by the listener thread and read by the typed projection
/// closure on the actor thread.
pub(crate) type EmbedSidecarSlot = Arc<Mutex<EmbedSidecarState>>;

/// Construct a new, empty [`EmbedSidecarSlot`].
pub(crate) fn new_embed_sidecar_slot() -> EmbedSidecarSlot {
    Arc::new(Mutex::new(EmbedSidecarState::default()))
}

/// Construct a fresh slot plus a listener-thread clone in one call, so the
/// runtime construction wiring stays a single line (the listener thread takes the
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

/// Resolve the current live `refs.event` rows into the envelope map consumed by
/// the `refs.event.envelopes` encoder.
fn resolve_events(
    rows: &BTreeMap<String, ClaimedEventRow>,
) -> BTreeMap<String, EmbeddedEventEnvelope> {
    let ctx = RenderContext::new();
    let mut map: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    for (primary_id, row) in rows {
        let event = row_to_kernel_event(row);
        let projection: EmbedKindProjection = resolve_embed_projection(&event, &ctx);
        map.insert(primary_id.clone(), build_envelope(primary_id, projection));
    }
    map
}

/// Called from the listener thread after every update frame.
///
/// Applies the `refs.event` row-delta typed sidecar from `frame_bytes`, resolves
/// the store's current live rows via `nmp-content`, and updates the derived
/// `refs.event.envelopes` map. Missing `refs.event` entries leave the prior
/// state intact; clear rows must arrive as explicit `refs.event` rows.
///
/// Silent no-ops on any decode failure (D6).
pub(crate) fn update_embed_sidecar_from_frame(frame_bytes: &[u8], slot: &EmbedSidecarSlot) {
    let Ok(envelope) = decode_snapshot_envelope(frame_bytes) else {
        return;
    };
    let Ok(projections) = decode_snapshot_typed_projections(frame_bytes) else {
        return;
    };

    let Some(refs_event_entry) = projections
        .iter()
        .find(|entry| entry.key == REFS_EVENT_KEY || entry.schema_id == REFS_EVENT_KEY)
    else {
        return;
    };

    if let Ok(mut state) = slot.lock() {
        state.ref_events.apply_sidecar(
            &refs_event_entry.payload,
            envelope.session_id,
            envelope.snapshot_epoch,
        );
        state.envelopes = resolve_events(&state.ref_events.events());
    }
}

/// Read the current resolved embed map from the slot, cloning it out under the
/// lock. Returns an empty map when the slot is `None` (first tick, before any
/// frame is processed) or when the mutex is poisoned (D6).
fn snapshot_map(slot: &EmbedSidecarSlot) -> BTreeMap<String, EmbeddedEventEnvelope> {
    slot.lock()
        .map(|state| state.envelopes.clone())
        .unwrap_or_default()
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
        key: EMBED_SIDECAR_PROJECTION_KEY.to_string(),
        schema_id: EMBED_SIDECAR_SCHEMA_ID.to_string(),
        schema_version: EMBED_SIDECAR_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(EMBED_SIDECAR_FILE_IDENTIFIER).into_owned(),
        payload: encode_ref_event_envelopes(&map),
        ..Default::default()
    }
}

/// Register the typed `refs.event.envelopes` derived projection on `app`,
/// reading the resolved map derived from the `refs.event` store.
///
/// The typed projection (`register_typed_snapshot_projection`) feeds the Chirp
/// typed-frame shell, which decodes the `NEMB` FlatBuffer and so needs ZERO
/// embed-resolution logic in Swift (closes the EmbedHost D0 violation #1283).
///
/// On the first tick (before the listener thread has processed any frame) the
/// slot is `None`; both closures emit empty (D1: always present). After the
/// first frame the slot holds the pre-resolved map. D8: the closure is a pure
/// `Mutex::lock` read + encode, non-blocking on the actor thread. D0: kind
/// dispatch lives in `nmp-content`; this is a thin reader/encoder.
///
/// ADR-0070 R6-S2: the typed projection now uses `TypedProjectionEmissionState`
/// to omit an unchanged `refs.event.envelopes` frame when the host has declared
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

    app.register_typed_snapshot_projection(
        nmp_ownership::DeclaredProjectionKey::framework(
            EMBED_SIDECAR_PROJECTION_KEY,
            "projection.refs.event.envelopes",
        ),
        move || {
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
        },
    );
}

// ── Unit tests ──────────────────────────────────────────────────────────────
// Extracted to sibling files to keep this file under the 500 LOC gate:
// - `embed_sidecar_tests.rs`    — resolve path + wire shape tests (pre-R6-S2).
// - `embed_sidecar_emission_tests.rs` — ADR-0070 R6-S2 cardinal-trap tests.

#[cfg(test)]
#[path = "embed_sidecar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "embed_sidecar_emission_tests.rs"]
mod emission_tests;
