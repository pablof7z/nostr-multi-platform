//! Typed FlatBuffers wire codec for [`crate::SearchResultsSnapshot`].
//!
//! The authoritative in-crate shape of a NIP-50 search-results projection is
//! the serde JSON of [`SearchResultsSnapshot`] (see `snapshot_json`); this
//! module adds the typed FlatBuffers (`N50S`) encoding the `open_search`
//! higher-order entrypoint registers as the per-session typed sidecar
//! (ADR-0072, S9). The schema (`crates/nmp-nip50/schema/search_results.fbs`)
//! mirrors the Rust snapshot field-for-field.
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
#[path = "generated/search_results_generated.rs"]
pub mod generated;

use flatbuffers::WIPOffset;

use generated::nmp::nip_50 as fb;

use crate::{SearchHit, SearchHitSource, SearchResultsSnapshot};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip50.search";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"N50S";
/// Wire schema version. Bump on any breaking change to `search_results.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode a [`SearchResultsSnapshot`] to typed FlatBuffers bytes (with the
/// `N50S` file identifier).
#[must_use]
pub fn encode_search_results_snapshot(snapshot: &SearchResultsSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let hits: Vec<WIPOffset<fb::SearchHit<'_>>> = snapshot
        .hits
        .iter()
        .map(|hit| encode_hit(&mut fbb, hit))
        .collect();
    let hits = fbb.create_vector(&hits);

    let root = fb::SearchResultsSnapshot::create(
        &mut fbb,
        &fb::SearchResultsSnapshotArgs { hits: Some(hits) },
    );
    fb::finish_search_results_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_hit<'b>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'b>,
    hit: &SearchHit,
) -> WIPOffset<fb::SearchHit<'b>> {
    // Pre-create all child offsets before opening the SearchHit table.
    let tags: Vec<WIPOffset<fb::TagRow<'_>>> = hit
        .tags
        .iter()
        .map(|row| {
            let cells: Vec<WIPOffset<&str>> = row.iter().map(|c| fbb.create_string(c)).collect();
            let cells = fbb.create_vector(&cells);
            fb::TagRow::create(fbb, &fb::TagRowArgs { cells: Some(cells) })
        })
        .collect();
    let tags = fbb.create_vector(&tags);

    let provenance: Vec<WIPOffset<&str>> = hit
        .relay_provenance
        .iter()
        .map(|r| fbb.create_string(r))
        .collect();
    let provenance = fbb.create_vector(&provenance);

    let id = fbb.create_string(&hit.id);
    let author = fbb.create_string(&hit.author);
    let content = fbb.create_string(&hit.content);

    let (is_cache, source_relay) = match &hit.source {
        SearchHitSource::Cache => (true, None),
        SearchHitSource::Relay(url) => (false, Some(fbb.create_string(url))),
    };

    fb::SearchHit::create(
        fbb,
        &fb::SearchHitArgs {
            id: Some(id),
            author: Some(author),
            kind: hit.kind,
            created_at: hit.created_at,
            content: Some(content),
            tags: Some(tags),
            relay_provenance: Some(provenance),
            is_cache,
            source_relay,
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by
/// [`encode_search_results_snapshot`]) back into a [`SearchResultsSnapshot`].
/// Returns an error string on any malformed input.
pub fn decode_search_results_snapshot(bytes: &[u8]) -> Result<SearchResultsSnapshot, String> {
    if bytes.len() < 8 || !fb::search_results_snapshot_buffer_has_identifier(bytes) {
        return Err("missing N50S file identifier".to_string());
    }
    let root = fb::root_as_search_results_snapshot(bytes)
        .map_err(|e| format!("not a valid SearchResultsSnapshot buffer: {e}"))?;

    let mut hits = Vec::new();
    if let Some(fb_hits) = root.hits() {
        hits.reserve(fb_hits.len());
        for hit in fb_hits.iter() {
            hits.push(decode_hit(hit)?);
        }
    }
    Ok(SearchResultsSnapshot { hits })
}

fn decode_hit(hit: fb::SearchHit<'_>) -> Result<SearchHit, String> {
    let id = hit
        .id()
        .ok_or_else(|| "SearchHit.id: missing required string".to_string())?
        .to_string();
    let author = hit
        .author()
        .ok_or_else(|| "SearchHit.author: missing required string".to_string())?
        .to_string();
    let content = hit.content().unwrap_or_default().to_string();

    let mut tags = Vec::new();
    if let Some(fb_tags) = hit.tags() {
        for row in fb_tags.iter() {
            let mut cells = Vec::new();
            if let Some(fb_cells) = row.cells() {
                for cell in fb_cells.iter() {
                    cells.push(cell.to_string());
                }
            }
            tags.push(cells);
        }
    }

    let mut relay_provenance = Vec::new();
    if let Some(fb_prov) = hit.relay_provenance() {
        for r in fb_prov.iter() {
            relay_provenance.push(r.to_string());
        }
    }

    let source = if hit.is_cache() {
        SearchHitSource::Cache
    } else {
        SearchHitSource::Relay(hit.source_relay().unwrap_or_default().to_string())
    };

    Ok(SearchHit {
        id,
        author,
        kind: hit.kind(),
        created_at: hit.created_at(),
        content,
        tags,
        relay_provenance,
        source,
    })
}

#[cfg(test)]
#[path = "search_results_fb_tests.rs"]
mod tests;
