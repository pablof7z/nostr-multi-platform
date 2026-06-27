//! Typed FlatBuffers wire codec for the `claimed_event_embeds` snapshot sidecar
//! (issue #1283 / ADR-0034 §embed-sidecar).
//!
//! This is the typed compatibility projection emitted under the historical
//! `claimed_event_embeds` key by `nmp-ffi`. The payload is derived from the
//! authoritative `refs.event` row store and carries a pre-resolved
//! `primary_id -> EmbeddedEventEnvelope` map so a typed-frame shell (Chirp)
//! never re-implements the `match kind` resolver in Swift. See
//! `schema/embed_sidecar.fbs` for the field map.
//!
//! The shape mirrors the existing resolver types
//! ([`EmbeddedEventEnvelope`](crate::embed_projection::EmbeddedEventEnvelope) /
//! [`EmbedKindProjection`](crate::embed_projection::EmbedKindProjection)) — never
//! a bespoke re-parse. Per-kind `content_tree` bodies are carried as the verbatim
//! [`ContentTreeWire`](crate::wire::ContentTreeWire) typed buffer (`NFCT` root)
//! via the existing [`encode_content_tree`](crate::wire::encode_content_tree)
//! codec, reused as an opaque-bytes unit (no schema `include`), exactly as
//! `longform_fb` carries the article body.
//!
//! Honours D6 (no panics): [`decode_claimed_event_embeds`] returns `Err(String)`
//! on any malformed input; there are no `unwrap`/`expect`/panicking operations on
//! the decode path.
//!
//! ## Module layout
//!
//! Mirrors the `longform_fb` precedent (a sibling-module directory): this `mod.rs`
//! root holds the generated-binding declaration, the wire constants, and the two
//! public entry points; the per-variant encode / decode halves live in
//! [`encode`] / [`decode`] submodules so no hand-authored file exceeds the
//! file-size cap (AGENTS.md). The `tests` submodule round-trips the whole codec.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/embed_sidecar_generated.rs` are
//! produced by `flatc` from `schema/embed_sidecar.fbs`. Regenerate only with the
//! workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`. The schema is self-contained:
//!
//! ```sh
//! flatc --rust -o crates/nmp-content/src/wire/generated \
//!       crates/nmp-content/schema/embed_sidecar.fbs
//! rustfmt --edition 2021 \
//!       crates/nmp-content/src/wire/generated/embed_sidecar_generated.rs
//! ```

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This single generated module — and only it — opts
// back into `unsafe`. No hand-written code in this crate uses `unsafe`.
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
#[path = "../generated/embed_sidecar_generated.rs"]
pub mod generated;

mod decode;
mod encode;

use std::collections::BTreeMap;

use crate::embed_projection::EmbeddedEventEnvelope;

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "claimed_event_embeds";
/// Snapshot-projection key the typed compatibility sidecar is emitted under.
pub const PROJECTION_KEY: &str = "claimed_event_embeds";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NEMB";
/// Wire schema version. Bump on any breaking change to `embed_sidecar.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// Encode the `claimed_event_embeds` projection (envelopes keyed by
/// `primary_id`) to typed FlatBuffers bytes (with the `NEMB` file identifier).
///
/// `entries` is encoded in [`BTreeMap`] (ascending-`primary_id`) order so the
/// `(key)`-keyed `entries` vector is sorted — a host may binary-search it by
/// `primary_id`.
#[must_use]
pub fn encode_claimed_event_embeds(entries: &BTreeMap<String, EmbeddedEventEnvelope>) -> Vec<u8> {
    encode::encode_claimed_event_embeds(entries)
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_claimed_event_embeds`])
/// back into a `primary_id -> EmbeddedEventEnvelope` map. Returns an error string
/// on any malformed input or missing required field.
pub fn decode_claimed_event_embeds(
    bytes: &[u8],
) -> Result<BTreeMap<String, EmbeddedEventEnvelope>, String> {
    decode::decode_claimed_event_embeds(bytes)
}

#[cfg(test)]
mod tests;
