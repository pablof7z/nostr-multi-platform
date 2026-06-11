//! Typed FlatBuffers wire codec for the kernel-owned `"active_account"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"active_account"`: the active account pubkey string (`account_snapshot().1`),
//! `null` when no account is active. This module adds a **typed FlatBuffers**
//! encoding of the same value, carried in the `typed_projections` sidecar
//! (ADR-0037) ALONGSIDE — never replacing — the generic `Value` projection.
//!
//! [`ActiveAccountModel`] is built from the same `account_snapshot().1` the JSON
//! path reads, in the same tick, so the two wire forms cannot diverge. The
//! typed entry is emitted unconditionally (mirroring the unconditional JSON
//! insertion); `has_active_account == false` mirrors JSON `null`.
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
#[path = "generated/active_account_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const ACTIVE_ACCOUNT_SCHEMA_ID: &str = "active_account";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const ACTIVE_ACCOUNT_FILE_IDENTIFIER: &[u8; 4] = b"KACT";
/// Wire schema version. Bump on any breaking change to `active_account.fbs`.
pub const ACTIVE_ACCOUNT_SCHEMA_VERSION: u32 = 1;

/// The `"active_account"` read model — the active account pubkey, `None` when no
/// account is active. Built from the same `account_snapshot().1` the JSON
/// projection reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActiveAccountModel {
    pub pubkey: Option<String>,
}

// --- encode ---------------------------------------------------------------

/// Encode an [`ActiveAccountModel`] to typed FlatBuffers bytes (with the `KACT`
/// file identifier).
#[must_use]
pub(crate) fn encode_active_account(model: &ActiveAccountModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let pubkey = model.pubkey.as_ref().map(|value| fbb.create_string(value));
    let root = fb::ActiveAccountSnapshot::create(
        &mut fbb,
        &fb::ActiveAccountSnapshotArgs {
            has_active_account: model.pubkey.is_some(),
            pubkey,
        },
    );
    fb::finish_active_account_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_active_account`])
/// back into an [`ActiveAccountModel`]. Returns an error string on any malformed
/// input.
pub fn decode_active_account(bytes: &[u8]) -> Result<ActiveAccountModel, String> {
    if bytes.len() < 8 || !fb::active_account_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KACT file identifier".to_string());
    }
    let root = fb::root_as_active_account_snapshot(bytes)
        .map_err(|e| format!("not a valid ActiveAccountSnapshot buffer: {e}"))?;

    Ok(ActiveAccountModel {
        pubkey: root
            .has_active_account()
            .then(|| root.pubkey().unwrap_or_default().to_string()),
    })
}

#[cfg(test)]
#[path = "active_account_fb_tests.rs"]
mod tests;
