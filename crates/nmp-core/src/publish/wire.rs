//! Typed FlatBuffers payload codec for the `nmp.publish` `ActionModule`
//! (ADR-0071 / S3 #1751).
//!
//! This is the WRITE-direction typed payload carried as the OPAQUE
//! `DispatchEnvelope.payload` for `action_namespace = "nmp.publish"`. The
//! transport (S2 / #1750) carries the bytes verbatim; the registry adapter
//! decodes them through [`ActionPayload::decode`] here — the SINGLE typed-decode
//! site — running the fail-closed `schema_version` gate BEFORE
//! `PublishModule::start()`.
//!
//! # Pre-signed events are not app-dispatchable
//!
//! This app-facing schema intentionally does not carry pre-signed events.
//! Externally signed/verbatim events remain supported through internal or
//! protocol-owned seams that call `PublishCommand::SignedEvent` and pass explicit
//! route provenance. Normal apps dispatch unsigned publish drafts and let the
//! actor finalize, sign, and publish.
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

mod selection;

use selection::{build_signer, build_target, read_signer, read_target};

use crate::publish::action::{PublishAction, PublishSigner, PublishTarget};
use crate::substrate::{ActionPayload, ActionPayloadDecodeError};

/// Stable identity of the `nmp.publish` typed payload schema.
pub const SCHEMA_ID: &str = "nmp.publish";
/// Wire schema version. Bump on any breaking change to `publish.fbs`.
pub const SCHEMA_VERSION: u32 = 4;
/// FlatBuffers file identifier embedded in every buffer this codec emits.
/// (Used by the round-trip tests + documents the wire magic; the generated
/// `publish_payload_buffer_has_identifier` is what the decode actually checks.)
// `allow(dead_code)`: asserted in the tests submodule's round-trip test;
// the per-crate lint sees only non-test callers when compiling without tests.
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

// --- encode ------------------------------------------------------------------

/// Encode a dispatchable [`PublishAction`] to typed FlatBuffers bytes.
///
/// The pre-signed `Publish` variant is not dispatchable through this app-facing
/// schema. `ActionPayload::encode` is infallible, so that variant returns an
/// invalid byte payload that fails closed at the registry decode gate.
#[must_use]
fn encode_publish_payload(action: &PublishAction) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let (body_type, body) = match action {
        PublishAction::Publish { .. } => return Vec::new(),
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
                &fb::PublishProfileArgs {
                    fields: Some(fields),
                },
            );
            (
                fb::PublishPayloadBody::PublishProfile,
                profile.as_union_value(),
            )
        }
        PublishAction::PublishRaw {
            kind,
            tags,
            content,
            target,
            signer,
        } => {
            let (raw, _) = build_publish_raw(&mut fbb, *kind, tags, content, target, signer);
            (fb::PublishPayloadBody::PublishRaw, raw.as_union_value())
        }
        PublishAction::PublishReply {
            content,
            reply_to_event_id,
            target,
            signer,
        } => {
            let content = fbb.create_string(content);
            let reply_to_event_id = fbb.create_string(reply_to_event_id);
            let target = build_target(&mut fbb, target);
            let signer = build_signer(&mut fbb, signer);
            let reply = fb::PublishReply::create(
                &mut fbb,
                &fb::PublishReplyArgs {
                    content: Some(content),
                    reply_to_event_id: Some(reply_to_event_id),
                    target: Some(target),
                    signer,
                },
            );
            (fb::PublishPayloadBody::PublishReply, reply.as_union_value())
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
    signer: &PublishSigner,
) -> (WIPOffset<fb::PublishRaw<'a>>, ()) {
    let tag_offsets: Vec<WIPOffset<fb::TagRow<'_>>> = tags
        .iter()
        .map(|row| {
            let values: Vec<WIPOffset<&str>> = row.iter().map(|s| fbb.create_string(s)).collect();
            let values = fbb.create_vector(&values);
            fb::TagRow::create(
                fbb,
                &fb::TagRowArgs {
                    values: Some(values),
                },
            )
        })
        .collect();
    let tags = fbb.create_vector(&tag_offsets);
    let content = fbb.create_string(content);
    let target = build_target(fbb, target);
    let signer = build_signer(fbb, signer);
    let raw = fb::PublishRaw::create(
        fbb,
        &fb::PublishRawArgs {
            kind,
            tags: Some(tags),
            content: Some(content),
            target: Some(target),
            signer,
        },
    );
    (raw, ())
}

// --- decode ------------------------------------------------------------------

pub(super) fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

/// Decode typed FlatBuffers bytes into a [`PublishAction`].
///
/// Runs the fail-closed `schema_version` gate FIRST: an unrecognised version is
/// [`ActionPayloadDecodeError::SchemaVersionMismatch`] and the body is NOT
/// inspected (ADR-0071 §1).
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
        fb::PublishPayloadBody::PublishProfile => {
            let profile = root
                .body_as_publish_profile()
                .ok_or_else(|| malformed("body_type=PublishProfile but body absent"))?;
            let mut fields = serde_json::Map::new();
            if let Some(rows) = profile.fields() {
                for row in rows.iter() {
                    fields.insert(
                        row.key().to_string(),
                        Value::String(row.value().to_string()),
                    );
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
            Ok(PublishAction::PublishRaw {
                kind: raw.kind(),
                tags,
                content: raw.content().to_string(),
                target: read_target(raw.target())?,
                signer: read_signer(raw.signer())?,
            })
        }
        fb::PublishPayloadBody::PublishReply => {
            let reply = root
                .body_as_publish_reply()
                .ok_or_else(|| malformed("body_type=PublishReply but body absent"))?;
            Ok(PublishAction::PublishReply {
                content: reply.content().to_string(),
                reply_to_event_id: reply.reply_to_event_id().to_string(),
                target: read_target(reply.target())?,
                signer: read_signer(reply.signer())?,
            })
        }
        other => Err(malformed(format!(
            "unknown PublishPayloadBody discriminant: {other:?}"
        ))),
    }
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
        &fb::PublishTargetArgs {
            explicit: false,
            relays: None,
            route_class: None,
        },
    );
    let raw = fb::PublishRaw::create(
        &mut fbb,
        &fb::PublishRawArgs {
            kind: 1,
            tags: None,
            content: Some(content),
            target: Some(target),
            signer: None,
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
