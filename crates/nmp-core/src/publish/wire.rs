//! Typed FlatBuffers payload codec for the `nmp.publish` `ActionModule`
//! (ADR-0064 / S3 #1751).
//!
//! This is the WRITE-direction typed payload carried as the OPAQUE
//! `DispatchEnvelope.payload` for `action_namespace = "nmp.publish"`. The
//! transport (S2 / #1750) carries the bytes verbatim; the registry adapter
//! decodes them through [`ActionPayload::decode`] here — the SINGLE typed-decode
//! site — running the fail-closed `schema_version` gate BEFORE
//! `PublishModule::start()`.
//!
//! # Opaque pre-signed event (signature byte-exactness)
//!
//! The pre-signed [`PublishAction::Publish`] variant carries the canonical
//! NIP-01 event as OPAQUE BYTES — the verbatim wire JSON object
//! `{ id, pubkey, created_at, kind, tags, content, sig }` produced by
//! [`SignedEvent::to_nip01_json`]. It is NEVER re-modelled as a typed table:
//! the `id`/`sig` commit to that exact serialization, so a typed-table
//! round-trip risks a byte-different re-encode that invalidates the signature.
//! [`encode`](ActionPayload::encode) stores the canonical bytes verbatim and
//! [`decode`](ActionPayload::decode) reconstructs the `SignedEvent` by parsing
//! them — `canonical_event` survives the round trip byte-for-byte.
//!
//! Honours D6 (no panics): decode returns a data-shaped
//! [`ActionPayloadDecodeError`] on any malformed input; no
//! `unwrap`/`expect`/panicking-index on the decode path.

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
#[path = "wire/generated/publish_generated.rs"]
pub mod generated;

use flatbuffers::WIPOffset;
use serde_json::Value;

use generated::nmp::publish as fb;

use crate::publish::action::{PublishAction, PublishTarget, RelayUrl};
use crate::substrate::{
    ActionPayload, ActionPayloadDecodeError, SignedEvent, UnsignedEvent,
};

/// Stable identity of the `nmp.publish` typed payload schema.
pub const SCHEMA_ID: &str = "nmp.publish";
/// Wire schema version. Bump on any breaking change to `publish.fbs`.
pub const SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier embedded in every buffer this codec emits.
/// (Used by the round-trip tests + documents the wire magic; the generated
/// `publish_payload_buffer_has_identifier` is what the decode actually checks.)
#[allow(dead_code)]
pub const FILE_IDENTIFIER: &[u8; 4] = b"NPUB";

impl ActionPayload for PublishAction {
    const SCHEMA_ID: &'static str = SCHEMA_ID;
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        encode_publish_payload(self)
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        decode_publish_payload(bytes)
    }
}

// --- target round-trip -------------------------------------------------------

fn build_target<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    target: &PublishTarget,
) -> WIPOffset<fb::PublishTarget<'a>> {
    let (explicit, relay_offsets) = match target {
        PublishTarget::Auto => (false, Vec::new()),
        PublishTarget::Explicit { relays } => (
            true,
            relays.iter().map(|r| fbb.create_string(r)).collect::<Vec<_>>(),
        ),
    };
    let relays = fbb.create_vector(&relay_offsets);
    fb::PublishTarget::create(
        fbb,
        &fb::PublishTargetArgs {
            explicit,
            relays: Some(relays),
        },
    )
}

fn read_target(target: fb::PublishTarget<'_>) -> PublishTarget {
    if !target.explicit() {
        return PublishTarget::Auto;
    }
    let relays: Vec<RelayUrl> = target
        .relays()
        .map(|v| v.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    PublishTarget::Explicit { relays }
}

// --- encode ------------------------------------------------------------------

/// Encode a dispatchable [`PublishAction`] to typed FlatBuffers bytes.
///
/// The engine-internal `Cancel` variant is NOT dispatchable through the action
/// seam (it rides the dedicated FFI symbol; `PublishModule::start` rejects it).
/// To keep `encode` total without inventing a wire shape for it, `Cancel` is
/// encoded as an empty `PublishRaw` placeholder — it never round-trips through
/// dispatch (a `Cancel` payload would be rejected by `start` after decode), so
/// this is unreachable on any real dispatch path.
#[must_use]
fn encode_publish_payload(action: &PublishAction) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let (body_type, body) = match action {
        PublishAction::Publish { handle, event, target } => {
            let handle = fbb.create_string(handle);
            // OPAQUE: the canonical NIP-01 wire JSON, verbatim. Byte-exact so the
            // signature stays valid (NEVER a typed table re-encode).
            let canonical = event.to_nip01_json();
            let canonical_event = fbb.create_vector(canonical.as_bytes());
            let target = build_target(&mut fbb, target);
            let signed = fb::PublishSigned::create(
                &mut fbb,
                &fb::PublishSignedArgs {
                    handle: Some(handle),
                    canonical_event: Some(canonical_event),
                    target: Some(target),
                },
            );
            (fb::PublishPayloadBody::PublishSigned, signed.as_union_value())
        }
        PublishAction::PublishProfile { fields } => {
            let field_offsets: Vec<WIPOffset<fb::ProfileField<'_>>> = fields
                .iter()
                .map(|(key, value)| {
                    let key = fbb.create_string(key);
                    let value = fbb.create_string(value.as_str().unwrap_or_default());
                    fb::ProfileField::create(
                        &mut fbb,
                        &fb::ProfileFieldArgs {
                            key: Some(key),
                            value: Some(value),
                        },
                    )
                })
                .collect();
            let fields = fbb.create_vector(&field_offsets);
            let profile = fb::PublishProfile::create(
                &mut fbb,
                &fb::PublishProfileArgs { fields: Some(fields) },
            );
            (fb::PublishPayloadBody::PublishProfile, profile.as_union_value())
        }
        PublishAction::PublishRaw { kind, tags, content, target, signer_pubkey } => {
            let (raw, _) = build_publish_raw(&mut fbb, *kind, tags, content, target, signer_pubkey);
            (fb::PublishPayloadBody::PublishRaw, raw.as_union_value())
        }
    };

    let payload = fb::PublishPayload::create(
        &mut fbb,
        &fb::PublishPayloadArgs {
            schema_version: SCHEMA_VERSION,
            body_type,
            body: Some(body),
        },
    );
    fb::finish_publish_payload_buffer(&mut fbb, payload);
    fbb.finished_data().to_vec()
}

#[allow(clippy::type_complexity)]
fn build_publish_raw<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    kind: u32,
    tags: &[Vec<String>],
    content: &str,
    target: &PublishTarget,
    signer_pubkey: &Option<String>,
) -> (WIPOffset<fb::PublishRaw<'a>>, ()) {
    let tag_offsets: Vec<WIPOffset<fb::TagRow<'_>>> = tags
        .iter()
        .map(|row| {
            let values: Vec<WIPOffset<&str>> =
                row.iter().map(|s| fbb.create_string(s)).collect();
            let values = fbb.create_vector(&values);
            fb::TagRow::create(fbb, &fb::TagRowArgs { values: Some(values) })
        })
        .collect();
    let tags = fbb.create_vector(&tag_offsets);
    let content = fbb.create_string(content);
    let target = build_target(fbb, target);
    let signer_pubkey = signer_pubkey.as_ref().map(|s| fbb.create_string(s));
    let raw = fb::PublishRaw::create(
        fbb,
        &fb::PublishRawArgs {
            kind,
            tags: Some(tags),
            content: Some(content),
            target: Some(target),
            signer_pubkey,
        },
    );
    (raw, ())
}

// --- decode ------------------------------------------------------------------

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed { reason: reason.into() }
}

/// Decode typed FlatBuffers bytes into a [`PublishAction`].
///
/// Runs the fail-closed `schema_version` gate FIRST: an unrecognised version is
/// [`ActionPayloadDecodeError::SchemaVersionMismatch`] and the body is NOT
/// inspected (ADR-0064 §1). The pre-signed `canonical_event` bytes are parsed
/// back into a `SignedEvent` (NOT re-modelled), preserving byte-exactness on a
/// subsequent `encode`.
fn decode_publish_payload(bytes: &[u8]) -> Result<PublishAction, ActionPayloadDecodeError> {
    if bytes.len() < 8 || !fb::publish_payload_buffer_has_identifier(bytes) {
        return Err(malformed("missing NPUB file identifier"));
    }
    let root = fb::root_as_publish_payload(bytes)
        .map_err(|e| malformed(format!("not a valid PublishPayload buffer: {e}")))?;

    // Gate FIRST — read the RAW version and reject before touching the body.
    let found = root.schema_version();
    if found != SCHEMA_VERSION {
        return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
            found,
            expected: SCHEMA_VERSION,
        });
    }

    match root.body_type() {
        fb::PublishPayloadBody::PublishSigned => {
            let signed = root
                .body_as_publish_signed()
                .ok_or_else(|| malformed("body_type=PublishSigned but body absent"))?;
            let handle = signed.handle().to_string();
            let canonical = signed.canonical_event().bytes();
            let event = parse_nip01_event(canonical)?;
            let target = read_target(signed.target());
            Ok(PublishAction::Publish { handle, event, target })
        }
        fb::PublishPayloadBody::PublishProfile => {
            let profile = root
                .body_as_publish_profile()
                .ok_or_else(|| malformed("body_type=PublishProfile but body absent"))?;
            let mut fields = serde_json::Map::new();
            if let Some(rows) = profile.fields() {
                for row in rows.iter() {
                    fields.insert(row.key().to_string(), Value::String(row.value().to_string()));
                }
            }
            Ok(PublishAction::PublishProfile { fields })
        }
        fb::PublishPayloadBody::PublishRaw => {
            let raw = root
                .body_as_publish_raw()
                .ok_or_else(|| malformed("body_type=PublishRaw but body absent"))?;
            let tags = raw
                .tags()
                .map(|rows| {
                    rows.iter()
                        .map(|row| {
                            row.values()
                                .map(|v| v.iter().map(|s| s.to_string()).collect())
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let signer_pubkey = raw.signer_pubkey().map(|s| s.to_string());
            Ok(PublishAction::PublishRaw {
                kind: raw.kind(),
                tags,
                content: raw.content().to_string(),
                target: read_target(raw.target()),
                signer_pubkey,
            })
        }
        other => Err(malformed(format!("unknown PublishPayloadBody discriminant: {other:?}"))),
    }
}

/// Parse the OPAQUE canonical NIP-01 wire JSON bytes back into a [`SignedEvent`].
///
/// The bytes are the verbatim `{ id, pubkey, created_at, kind, tags, content,
/// sig }` object [`SignedEvent::to_nip01_json`] produces. Parsing (never
/// re-modelling) keeps the signature byte-exact on a subsequent `encode`.
fn parse_nip01_event(bytes: &[u8]) -> Result<SignedEvent, ActionPayloadDecodeError> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| malformed(format!("canonical_event is not valid NIP-01 JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| malformed("canonical_event is not a JSON object"))?;

    let get_str = |k: &str| -> Result<String, ActionPayloadDecodeError> {
        obj.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| malformed(format!("canonical_event missing string field '{k}'")))
    };
    let id = get_str("id")?;
    let sig = get_str("sig")?;
    let pubkey = get_str("pubkey")?;
    let content = get_str("content")?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed("canonical_event missing numeric 'kind'"))? as u32;
    let created_at = obj
        .get("created_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed("canonical_event missing numeric 'created_at'"))?;
    let tags = obj
        .get("tags")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row.as_array()
                        .map(|cols| {
                            cols.iter()
                                .filter_map(|c| c.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(SignedEvent {
        id,
        sig,
        unsigned: UnsignedEvent { pubkey, kind, tags, content, created_at },
    })
}

/// Test-only: a finished `nmp.publish` payload buffer carrying an arbitrary
/// `schema_version` (used by the registry's fail-closed trip tests to prove the
/// version gate rejects BEFORE `start()`). Encodes a minimal `PublishRaw` body.
#[cfg(test)]
#[must_use]
pub(crate) fn encode_with_schema_version_for_test(schema_version: u32) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let content = fbb.create_string("x");
    let target = fb::PublishTarget::create(
        &mut fbb,
        &fb::PublishTargetArgs { explicit: false, relays: None },
    );
    let raw = fb::PublishRaw::create(
        &mut fbb,
        &fb::PublishRawArgs {
            kind: 1,
            tags: None,
            content: Some(content),
            target: Some(target),
            signer_pubkey: None,
        },
    );
    let payload = fb::PublishPayload::create(
        &mut fbb,
        &fb::PublishPayloadArgs {
            schema_version,
            body_type: fb::PublishPayloadBody::PublishRaw,
            body: Some(raw.as_union_value()),
        },
    );
    fb::finish_publish_payload_buffer(&mut fbb, payload);
    fbb.finished_data().to_vec()
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
