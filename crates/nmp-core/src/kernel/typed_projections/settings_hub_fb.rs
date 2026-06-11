//! Typed FlatBuffers wire codec for the kernel-owned `"settings_hub"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"settings_hub"`: `{ "relay_count": configured_relays_snapshot().len() }`.
//! This module adds a **typed FlatBuffers** encoding of the same shape, carried
//! in the `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing —
//! the generic `Value` projection.
//!
//! [`SettingsHubModel`] is built from the same `configured_relays_snapshot()`
//! length the JSON path reads, in the same tick, so the two wire forms cannot
//! diverge.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
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
#[path = "generated/settings_hub_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const SETTINGS_HUB_SCHEMA_ID: &str = "settings_hub";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const SETTINGS_HUB_FILE_IDENTIFIER: &[u8; 4] = b"KSHB";
/// Wire schema version. Bump on any breaking change to `settings_hub.fbs`.
pub const SETTINGS_HUB_SCHEMA_VERSION: u32 = 1;

/// The `"settings_hub"` read model — the configured relay count. Built from the
/// same `configured_relays_snapshot().len()` the JSON projection reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingsHubModel {
    pub relay_count: u32,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`SettingsHubModel`] to typed FlatBuffers bytes (with the `KSHB`
/// file identifier).
#[must_use]
pub(crate) fn encode_settings_hub(model: &SettingsHubModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let root = fb::SettingsHubSnapshot::create(
        &mut fbb,
        &fb::SettingsHubSnapshotArgs {
            relay_count: model.relay_count,
        },
    );
    fb::finish_settings_hub_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_settings_hub`]) back
/// into a [`SettingsHubModel`]. Returns an error string on any malformed input.
pub fn decode_settings_hub(bytes: &[u8]) -> Result<SettingsHubModel, String> {
    if bytes.len() < 8 || !fb::settings_hub_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KSHB file identifier".to_string());
    }
    let root = fb::root_as_settings_hub_snapshot(bytes)
        .map_err(|e| format!("not a valid SettingsHubSnapshot buffer: {e}"))?;

    Ok(SettingsHubModel {
        relay_count: root.relay_count(),
    })
}

#[cfg(test)]
#[path = "settings_hub_fb_tests.rs"]
mod tests;
