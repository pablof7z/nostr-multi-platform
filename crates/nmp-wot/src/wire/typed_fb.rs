//! Typed FlatBuffers wire codec for [`crate::runtime::WotBootstrapSnapshot`].
//!
//! The authoritative FFI shape of the `nmp.wot.bootstrap` projection is the
//! serde JSON of [`WotBootstrapSnapshot`] (registered via
//! `register_snapshot_projection` in `crate::runtime::register_runtime`). This
//! module adds a **typed FlatBuffers** encoding of the same struct — a
//! self-describing, schema-versioned, language-neutral binary the host
//! platforms (Swift / Kotlin / TypeScript) can decode with generated accessors
//! instead of JSON reflection. It is a sidecar codec: the serde shape stays
//! authoritative; this is the typed payload carried in the `typed_projections`
//! sidecar (ADR-0072, `crates/nmp-core/schema/nmp_update.fbs`).
//!
//! The schema (`crates/nmp-wot/schema/wot_bootstrap.fbs`) mirrors the Rust
//! struct field-for-field. `active_pubkey: Option<String>` carries a
//! `has_active_pubkey` presence flag plus the value so absent (`None`)
//! round-trips distinctly from a present-empty string — the same optional-field
//! convention used by `content_tree.fbs` / `wallet_status.fbs`. The `usize`
//! counts are carried as `uint64` (FlatBuffers has no `usize`); the casts are
//! lossless because the counts are non-negative.
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
#[path = "generated/wot_bootstrap_generated.rs"]
pub mod generated;

use generated::nmp::wot as fb;

use crate::runtime::WotBootstrapSnapshot;
use nmp_core::TypedProjectionData;

/// Host-declared projection key this typed payload is emitted under.
pub const PROJECTION_KEY: &str = "nmp.wot.bootstrap";
/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.wot.bootstrap";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NWBS";
/// Wire schema version. Bump on any breaking change to `wot_bootstrap.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- typed-projection envelope -------------------------------------------

/// Build the [`TypedProjectionData`] sidecar entry for a snapshot — the value
/// `register_typed_snapshot_projection`'s closure returns and the kernel
/// collects into a frame's `typed_projections` sidecar.
#[must_use]
pub fn typed_projection(snapshot: &WotBootstrapSnapshot) -> TypedProjectionData {
    TypedProjectionData {
        key: PROJECTION_KEY.to_string(),
        schema_id: SCHEMA_ID.to_string(),
        schema_version: SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(),
        payload: encode_wot_bootstrap(snapshot),
        ..Default::default()
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`WotBootstrapSnapshot`] to typed FlatBuffers bytes (with the
/// `NWBS` file identifier).
#[must_use]
pub fn encode_wot_bootstrap(snapshot: &WotBootstrapSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    // All string offsets must be created before the table is started.
    let active_pubkey = snapshot
        .active_pubkey
        .as_ref()
        .map(|s| fbb.create_string(s));

    let root = fb::WotBootstrapSnapshot::create(
        &mut fbb,
        &fb::WotBootstrapSnapshotArgs {
            has_active_pubkey: snapshot.active_pubkey.is_some(),
            active_pubkey,
            active_follow_count: snapshot.active_follow_count as u64,
            bootstrap_requested: snapshot.bootstrap_requested,
            graph_follow_authors: snapshot.graph_follow_authors as u64,
            graph_mute_authors: snapshot.graph_mute_authors as u64,
        },
    );
    fb::finish_wot_bootstrap_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_wot_bootstrap`])
/// back into a [`WotBootstrapSnapshot`]. Returns an error string on any
/// malformed input.
pub fn decode_wot_bootstrap(bytes: &[u8]) -> Result<WotBootstrapSnapshot, String> {
    if bytes.len() < 8 || !fb::wot_bootstrap_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NWBS file identifier".to_string());
    }
    let root = fb::root_as_wot_bootstrap_snapshot(bytes)
        .map_err(|e| format!("not a valid WotBootstrapSnapshot buffer: {e}"))?;

    Ok(WotBootstrapSnapshot {
        active_pubkey: optional_string(root.has_active_pubkey(), root.active_pubkey()),
        active_follow_count: root.active_follow_count() as usize,
        bootstrap_requested: root.bootstrap_requested(),
        graph_follow_authors: root.graph_follow_authors() as usize,
        graph_mute_authors: root.graph_mute_authors() as usize,
    })
}

/// Reconstruct an `Option<String>` from a `has_*` flag + the wire string,
/// distinguishing absent (`None`) from present-empty (`Some("")`).
fn optional_string(present: bool, value: Option<&str>) -> Option<String> {
    if present {
        Some(value.unwrap_or_default().to_string())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "typed_fb_tests.rs"]
mod tests;
