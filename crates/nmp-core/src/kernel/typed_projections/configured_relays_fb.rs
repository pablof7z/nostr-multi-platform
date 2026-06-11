//! Typed FlatBuffers wire codec for the kernel-owned `"configured_relays"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"configured_relays"`: the serialisation of `configured_relays_snapshot()`,
//! a slice of [`AppRelay`](crate::kernel::AppRelay) (`{ url, role }`). This
//! module adds a **typed FlatBuffers** encoding of the same shape, carried in
//! the `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection.
//!
//! [`ConfiguredRelaysModel`] is built directly from the same `AppRelay` slice
//! the JSON path serialises, in the same tick, so the two wire forms cannot
//! structurally diverge.
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
#[path = "generated/configured_relays_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const CONFIGURED_RELAYS_SCHEMA_ID: &str = "configured_relays";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const CONFIGURED_RELAYS_FILE_IDENTIFIER: &[u8; 4] = b"KCRL";
/// Wire schema version. Bump on any breaking change to `configured_relays.fbs`.
pub const CONFIGURED_RELAYS_SCHEMA_VERSION: u32 = 1;

/// One configured relay row — a field-for-field mirror of one
/// [`AppRelay`](crate::kernel::AppRelay) (`url` + canonicalised `role`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfiguredRelayRow {
    pub url: String,
    pub role: String,
}

/// The `"configured_relays"` read model — the ordered relay rows. Built from the
/// same `AppRelay` slice the JSON projection serialises (see
/// [`From<&[crate::kernel::AppRelay]>`]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfiguredRelaysModel {
    pub relays: Vec<ConfiguredRelayRow>,
}

impl From<&[crate::kernel::AppRelay]> for ConfiguredRelaysModel {
    fn from(rows: &[crate::kernel::AppRelay]) -> Self {
        Self {
            relays: rows
                .iter()
                .map(|relay| ConfiguredRelayRow {
                    url: relay.url().to_string(),
                    role: relay.role().to_string(),
                })
                .collect(),
        }
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ConfiguredRelaysModel`] to typed FlatBuffers bytes (with the
/// `KCRL` file identifier). Row order is preserved verbatim.
#[must_use]
pub(crate) fn encode_configured_relays(model: &ConfiguredRelaysModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let row_offsets: Vec<WIPOffset<fb::ConfiguredRelay>> = model
        .relays
        .iter()
        .map(|row| {
            let url = fbb.create_string(&row.url);
            let role = fbb.create_string(&row.role);
            fb::ConfiguredRelay::create(
                &mut fbb,
                &fb::ConfiguredRelayArgs {
                    url: Some(url),
                    role: Some(role),
                },
            )
        })
        .collect();
    let relays = fbb.create_vector(&row_offsets);

    let root = fb::ConfiguredRelaysSnapshot::create(
        &mut fbb,
        &fb::ConfiguredRelaysSnapshotArgs {
            relays: Some(relays),
        },
    );
    fb::finish_configured_relays_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_configured_relays`])
/// back into a [`ConfiguredRelaysModel`]. Returns an error string on any
/// malformed input.
pub fn decode_configured_relays(bytes: &[u8]) -> Result<ConfiguredRelaysModel, String> {
    if bytes.len() < 8 || !fb::configured_relays_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KCRL file identifier".to_string());
    }
    let root = fb::root_as_configured_relays_snapshot(bytes)
        .map_err(|e| format!("not a valid ConfiguredRelaysSnapshot buffer: {e}"))?;

    let mut relays = Vec::new();
    if let Some(fb_relays) = root.relays() {
        relays.reserve(fb_relays.len());
        for relay in fb_relays.iter() {
            relays.push(ConfiguredRelayRow {
                url: relay.url().unwrap_or_default().to_string(),
                role: relay.role().unwrap_or_default().to_string(),
            });
        }
    }

    Ok(ConfiguredRelaysModel { relays })
}

#[cfg(test)]
#[path = "configured_relays_fb_tests.rs"]
mod tests;
