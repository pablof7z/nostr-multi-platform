//! Typed FlatBuffers wire codec for the kernel-owned `"relay_role_options"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"relay_role_options"`: the serialisation of
//! `crate::actor::relay_role_options()`, a `Vec<RelayRoleOption>`
//! (`{ value, label, tint, is_default }`). This module adds a **typed
//! FlatBuffers** encoding of the same shape, carried in the `typed_projections`
//! sidecar (ADR-0037) ALONGSIDE — never replacing — the generic `Value`
//! projection.
//!
//! [`RelayRoleOptionsModel`] is built directly from the same option vector the
//! JSON path serialises, in the same tick, so the two wire forms cannot
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
#[path = "generated/relay_role_options_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub(crate) const RELAY_ROLE_OPTIONS_SCHEMA_ID: &str = "relay_role_options";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const RELAY_ROLE_OPTIONS_FILE_IDENTIFIER: &[u8; 4] = b"KRRO";
/// Wire schema version. Bump on any breaking change to `relay_role_options.fbs`.
pub(crate) const RELAY_ROLE_OPTIONS_SCHEMA_VERSION: u32 = 1;

/// One relay-role picker option — a field-for-field mirror of one
/// `RelayRoleOption`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RelayRoleOptionRow {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) tint: String,
    pub(crate) is_default: bool,
}

/// The `"relay_role_options"` read model — the ordered picker options. Built
/// from the same option vector the JSON projection serialises; see
/// [`Kernel::builtin_typed_projections`](crate::kernel::Kernel), which maps the
/// `crate::actor::relay_role_options()` output (whose element type is
/// `pub(crate)`-but-unnameable outside the codegen feature) into these rows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RelayRoleOptionsModel {
    pub(crate) options: Vec<RelayRoleOptionRow>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`RelayRoleOptionsModel`] to typed FlatBuffers bytes (with the
/// `KRRO` file identifier). Option order is preserved verbatim.
#[must_use]
pub(crate) fn encode_relay_role_options(model: &RelayRoleOptionsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let option_offsets: Vec<WIPOffset<fb::RelayRoleOption>> = model
        .options
        .iter()
        .map(|option| {
            let value = fbb.create_string(&option.value);
            let label = fbb.create_string(&option.label);
            let tint = fbb.create_string(&option.tint);
            fb::RelayRoleOption::create(
                &mut fbb,
                &fb::RelayRoleOptionArgs {
                    value: Some(value),
                    label: Some(label),
                    tint: Some(tint),
                    is_default: option.is_default,
                },
            )
        })
        .collect();
    let options = fbb.create_vector(&option_offsets);

    let root = fb::RelayRoleOptionsSnapshot::create(
        &mut fbb,
        &fb::RelayRoleOptionsSnapshotArgs {
            options: Some(options),
        },
    );
    fb::finish_relay_role_options_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_relay_role_options`])
/// back into a [`RelayRoleOptionsModel`]. Returns an error string on any
/// malformed input.
#[cfg(test)]
pub(crate) fn decode_relay_role_options(bytes: &[u8]) -> Result<RelayRoleOptionsModel, String> {
    if bytes.len() < 8 || !fb::relay_role_options_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KRRO file identifier".to_string());
    }
    let root = fb::root_as_relay_role_options_snapshot(bytes)
        .map_err(|e| format!("not a valid RelayRoleOptionsSnapshot buffer: {e}"))?;

    let mut options = Vec::new();
    if let Some(fb_options) = root.options() {
        options.reserve(fb_options.len());
        for option in fb_options.iter() {
            options.push(RelayRoleOptionRow {
                value: option.value().unwrap_or_default().to_string(),
                label: option.label().unwrap_or_default().to_string(),
                tint: option.tint().unwrap_or_default().to_string(),
                is_default: option.is_default(),
            });
        }
    }

    Ok(RelayRoleOptionsModel { options })
}

#[cfg(test)]
#[path = "relay_role_options_fb_tests.rs"]
mod tests;
