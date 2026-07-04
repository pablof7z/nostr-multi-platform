//! Typed FlatBuffers wire codec for [`crate::AdCollectionSnapshot`].
//!
//! The authoritative in-crate shape is the serde JSON of
//! [`AdCollectionSnapshot`]; this module adds the typed FlatBuffers (`ADCL`)
//! encoding the `open_ad_collection` doorway registers as the per-session typed
//! sidecar (ADR-0072, S9). The schema
//! (`crates/nmp-nip-ad/schema/ad_collection.fbs`) mirrors the Rust snapshot
//! field-for-field.
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
#[path = "generated/ad_collection_generated.rs"]
pub mod generated;

use flatbuffers::WIPOffset;

use generated::nmp::nip_ad as fb;

use crate::{AdCollectionRow, AdCollectionSnapshot};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip-ad.collection";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"ADCL";
/// Wire schema version. Bump on any breaking change to `ad_collection.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode an [`AdCollectionSnapshot`] to typed FlatBuffers bytes (with the
/// `ADCL` file identifier).
#[must_use]
pub fn encode_ad_collection_snapshot(snapshot: &AdCollectionSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let rows: Vec<WIPOffset<fb::AdCollectionRow<'_>>> = snapshot
        .rows
        .iter()
        .map(|row| encode_row(&mut fbb, row))
        .collect();
    let rows = fbb.create_vector(&rows);

    let root = fb::AdCollectionSnapshot::create(
        &mut fbb,
        &fb::AdCollectionSnapshotArgs { rows: Some(rows) },
    );
    fb::finish_ad_collection_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_row<'b>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'b>,
    row: &AdCollectionRow,
) -> WIPOffset<fb::AdCollectionRow<'b>> {
    // Pre-create all child offsets before opening the AdCollectionRow table.
    let tags: Vec<WIPOffset<fb::TagRow<'_>>> = row
        .tags
        .iter()
        .map(|cells| {
            let cells: Vec<WIPOffset<&str>> = cells.iter().map(|c| fbb.create_string(c)).collect();
            let cells = fbb.create_vector(&cells);
            fb::TagRow::create(fbb, &fb::TagRowArgs { cells: Some(cells) })
        })
        .collect();
    let tags = fbb.create_vector(&tags);

    let provenance: Vec<WIPOffset<&str>> = row
        .relay_provenance
        .iter()
        .map(|r| fbb.create_string(r))
        .collect();
    let provenance = fbb.create_vector(&provenance);

    let id = fbb.create_string(&row.id);
    let author = fbb.create_string(&row.author);
    let content = fbb.create_string(&row.content);

    fb::AdCollectionRow::create(
        fbb,
        &fb::AdCollectionRowArgs {
            id: Some(id),
            author: Some(author),
            kind: row.kind,
            created_at: row.created_at,
            content: Some(content),
            tags: Some(tags),
            relay_provenance: Some(provenance),
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by
/// [`encode_ad_collection_snapshot`]) back into an [`AdCollectionSnapshot`].
/// Returns an error string on any malformed input.
pub fn decode_ad_collection_snapshot(bytes: &[u8]) -> Result<AdCollectionSnapshot, String> {
    if bytes.len() < 8 || !fb::ad_collection_snapshot_buffer_has_identifier(bytes) {
        return Err("missing ADCL file identifier".to_string());
    }
    let root = fb::root_as_ad_collection_snapshot(bytes)
        .map_err(|e| format!("not a valid AdCollectionSnapshot buffer: {e}"))?;

    let mut rows = Vec::new();
    if let Some(fb_rows) = root.rows() {
        rows.reserve(fb_rows.len());
        for row in fb_rows.iter() {
            rows.push(decode_row(row)?);
        }
    }
    Ok(AdCollectionSnapshot { rows })
}

fn decode_row(row: fb::AdCollectionRow<'_>) -> Result<AdCollectionRow, String> {
    let id = row
        .id()
        .ok_or_else(|| "AdCollectionRow.id: missing required string".to_string())?
        .to_string();
    let author = row
        .author()
        .ok_or_else(|| "AdCollectionRow.author: missing required string".to_string())?
        .to_string();
    let content = row.content().unwrap_or_default().to_string();

    let mut tags = Vec::new();
    if let Some(fb_tags) = row.tags() {
        for tag_row in fb_tags.iter() {
            let mut cells = Vec::new();
            if let Some(fb_cells) = tag_row.cells() {
                for cell in fb_cells.iter() {
                    cells.push(cell.to_string());
                }
            }
            tags.push(cells);
        }
    }

    let mut relay_provenance = Vec::new();
    if let Some(fb_prov) = row.relay_provenance() {
        for r in fb_prov.iter() {
            relay_provenance.push(r.to_string());
        }
    }

    Ok(AdCollectionRow {
        id,
        author,
        kind: row.kind(),
        created_at: row.created_at(),
        content,
        tags,
        relay_provenance,
    })
}

#[cfg(test)]
#[path = "ad_collection_fb_tests.rs"]
mod tests;
