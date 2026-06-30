//! Typed FlatBuffers wire encoding for `nmp_nip01::ModularTimelineSnapshot`.
//!
//! This schema carries NIP-10 timeline blocks only. Concrete feed row/card
//! payloads are owned by feed composition crates.

pub(crate) mod decode;
pub(crate) mod encode;

use crate::timeline_projection::ModularTimelineSnapshot;
pub(super) use crate::timeline_snapshot_generated::nmp::nip_01 as fb;

/// Stable projection identifier this wire shape projects into.
pub const SCHEMA_ID: &str = "nmp.nip01.timeline";

/// FlatBuffers file identifier for a `ModularTimelineSnapshot` root buffer.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NFTS";

/// Schema version of the typed timeline-snapshot payload.
pub const SCHEMA_VERSION: u32 = 3;

#[must_use]
pub fn encode_modular_timeline_snapshot(snapshot: &ModularTimelineSnapshot) -> Vec<u8> {
    encode::encode_modular_timeline_snapshot(snapshot)
}

pub fn decode_modular_timeline_snapshot(bytes: &[u8]) -> Result<ModularTimelineSnapshot, String> {
    decode::decode_modular_timeline_snapshot(bytes)
}

#[cfg(test)]
#[path = "typed_wire/tests.rs"]
mod tests;
