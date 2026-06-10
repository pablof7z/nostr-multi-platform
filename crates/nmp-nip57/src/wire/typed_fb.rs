//! Typed FlatBuffers wire codec for [`crate::projection::ZapsAggregateSnapshot`].
//!
//! The authoritative FFI shape of the `"nmp.nip57.zaps"` projection is the
//! serde JSON of [`ZapsAggregateSnapshot`] (registered via
//! `register_snapshot_projection` in `apps/chirp/.../ffi/register.rs`). This
//! module adds a **typed FlatBuffers** encoding of the same snapshot — a
//! self-describing, schema-versioned, language-neutral binary the host
//! platforms (Swift / Kotlin / TypeScript) can decode with generated accessors
//! instead of JSON reflection. It is a sidecar codec: the serde shape stays
//! authoritative; this is the typed payload carried in the `typed_projections`
//! sidecar (ADR-0037, `crates/nmp-core/schema/nmp_update.fbs`).
//!
//! The schema (`crates/nmp-nip57/schema/zaps.fbs`) mirrors the Rust snapshot
//! field-for-field. FlatBuffers has no map type, so the `totals` HashMap is
//! flattened to a `[ZapTotal]` vector, sorted by `target_event_id` for a stable
//! wire (HashMap iteration order is non-deterministic; decode rebuilds the map,
//! so order is immaterial to equality but determinism keeps frame bytes stable).
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This `allow` block scopes the relaxation to the
// single generated module — no hand-written code in this file uses `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/zaps_generated.rs"]
pub mod generated;

use flatbuffers::WIPOffset;

use generated::nmp::nip_57 as fb;

use crate::projection::{ZapCount, ZapsAggregateSnapshot};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip57.zaps";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NZAP";
/// Wire schema version. Bump on any breaking change to `zaps.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode a [`ZapsAggregateSnapshot`] to typed FlatBuffers bytes (with the
/// `NZAP` file identifier).
#[must_use]
pub fn encode_zaps_snapshot(snapshot: &ZapsAggregateSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    // Flatten the HashMap into a deterministically-ordered vector so the frame
    // bytes are stable across ticks with identical content.
    let mut entries: Vec<(&String, &ZapCount)> = snapshot.totals.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let totals: Vec<WIPOffset<fb::ZapTotal<'_>>> = entries
        .iter()
        .map(|(target, count)| {
            let target_event_id = fbb.create_string(target);
            fb::ZapTotal::create(
                &mut fbb,
                &fb::ZapTotalArgs {
                    target_event_id: Some(target_event_id),
                    total_msats: count.total_msats,
                    count: count.count,
                },
            )
        })
        .collect();
    let totals = fbb.create_vector(&totals);

    let root = fb::ZapsSnapshot::create(
        &mut fbb,
        &fb::ZapsSnapshotArgs {
            totals: Some(totals),
        },
    );
    fb::finish_zaps_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_zaps_snapshot`])
/// back into a [`ZapsAggregateSnapshot`]. Returns an error string on any
/// malformed input.
pub fn decode_zaps_snapshot(bytes: &[u8]) -> Result<ZapsAggregateSnapshot, String> {
    if bytes.len() < 8 || !fb::zaps_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NZAP file identifier".to_string());
    }
    let root = fb::root_as_zaps_snapshot(bytes)
        .map_err(|e| format!("not a valid ZapsSnapshot buffer: {e}"))?;

    let mut snapshot = ZapsAggregateSnapshot::empty();
    if let Some(totals) = root.totals() {
        for entry in totals.iter() {
            let target = entry
                .target_event_id()
                .ok_or_else(|| "ZapTotal.target_event_id: missing required string".to_string())?
                .to_string();
            snapshot.totals.insert(
                target,
                ZapCount {
                    total_msats: entry.total_msats(),
                    count: entry.count(),
                },
            );
        }
    }
    Ok(snapshot)
}

#[cfg(test)]
#[path = "typed_fb_tests.rs"]
mod tests;
