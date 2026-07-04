//! Typed FlatBuffers wire codec for the `refs.event.envelopes` snapshot sidecar
//! (issue #1283 / ADR-0072 §embed-sidecar).
//!
//! The payload is derived from the authoritative `refs.event` row store and
//! carries a pre-resolved `primary_id -> EmbeddedEventEnvelope` map so a
//! typed-frame shell never re-implements the `match kind` resolver in native
//! code. See
//! `schema/embed_sidecar.fbs` for the field map.
//!
//! The shape mirrors the existing resolver types
//! ([`EmbeddedEventEnvelope`](crate::embed_projection::EmbeddedEventEnvelope) /
//! [`EmbedKindProjection`](crate::embed_projection::EmbedKindProjection)) — never
//! a bespoke re-parse. Per-kind `content_tree` bodies are carried as the verbatim
//! [`ContentTreeWire`](crate::wire::ContentTreeWire) typed buffer (`NFCT` root)
//! via the existing [`encode_content_tree`](crate::wire::encode_content_tree)
//! codec, reused as an opaque-bytes unit (no schema `include`), exactly as
//! `nmp-nip23` carries the article body in the long-form sidecar.
//!
//! Honours D6 (no panics): [`decode_ref_event_envelopes`] returns `Err(String)`
//! on any malformed input; there are no `unwrap`/`expect`/panicking operations on
//! the decode path.
//!
//! ## Module layout
//!
//! Mirrors the typed-sidecar precedent (a sibling-module directory): this `mod.rs`
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
pub const SCHEMA_ID: &str = "refs.event.envelopes";
/// Snapshot-projection key the typed derived sidecar is emitted under.
pub const PROJECTION_KEY: &str = "refs.event.envelopes";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NEMB";
/// Wire schema version. Bump on any breaking change to `embed_sidecar.fbs`.
/// v3 (#2514): dropped author `display_name`/`picture` from the ShortNote /
/// Article / Highlight / Unknown tables — non-`Profile` projections carry raw
/// `author_pubkey` only; author display joins reactively at L5.
/// v4 (#3016): v3 removed those fields OUTRIGHT from the middle of each table,
/// which reflows every subsequent field's FlatBuffers vtable offset (a
/// non-additive schema change — `title`/`hero_image_url`/`created_at`/
/// `content_tree` etc. all silently moved). Restored them as `(deprecated)`
/// placeholders at their original position so the vtable layout for every
/// field that comes after them is stable again; no Rust-visible shape change
/// (deprecated fields generate no accessor).
pub const SCHEMA_VERSION: u32 = 4;

/// Encode the `refs.event.envelopes` projection (envelopes keyed by
/// `primary_id`) to typed FlatBuffers bytes (with the `NEMB` file identifier).
///
/// `entries` is encoded in [`BTreeMap`] (ascending-`primary_id`) order so the
/// `(key)`-keyed `entries` vector is sorted — a host may binary-search it by
/// `primary_id`.
#[must_use]
pub fn encode_ref_event_envelopes(entries: &BTreeMap<String, EmbeddedEventEnvelope>) -> Vec<u8> {
    encode::encode_ref_event_envelopes(entries)
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_ref_event_envelopes`])
/// back into a `primary_id -> EmbeddedEventEnvelope` map. Returns an error string
/// on any malformed input or missing required field.
pub fn decode_ref_event_envelopes(
    bytes: &[u8],
) -> Result<BTreeMap<String, EmbeddedEventEnvelope>, String> {
    decode::decode_ref_event_envelopes(bytes)
}

#[cfg(test)]
mod tests;
