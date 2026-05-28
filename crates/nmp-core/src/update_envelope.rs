//! Canonical FlatBuffers update frames for the single kernel→host channel.
//!
//! Every runtime frame is a binary `nmp.transport.UpdateFrame` with file
//! identifier `NMPU`. The frame has exactly two variants:
//!
//! - `Snapshot`: carries the full `KernelSnapshot` as a FlatBuffers value tree.
//! - `Panic`: terminal actor-thread death signal.
//!
//! The payload is deliberately not a JSON string embedded in a binary wrapper.
//! Host-extensible projections still need a generic value representation, so
//! the schema models JSON-like primitives as FlatBuffers tables instead of
//! pinning every app projection into `nmp-core`.

use crate::transport::wire as fb;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use serde_json::{Map, Number, Value};
use std::fmt;

/// Schema version of the periodic snapshot payload. Bump on any breaking
/// snapshot field change.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Owned bytes for one FlatBuffers `nmp.transport.UpdateFrame`.
pub type UpdateFrameBytes = Vec<u8>;

/// Actor-thread death payload. Terminal: hosts must stop sending commands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanicFrame {
    pub msg: String,
}

/// Decoded view used by Rust consumers and tests. Runtime transport remains
/// FlatBuffers bytes; this enum is not the wire shape.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEnvelope {
    Snapshot(Value),
    Panic(PanicFrame),
}

/// Owned, decoded form of one `nmp.transport.TypedProjection` sidecar entry.
///
/// The `payload` is opaque to `nmp-core`: it is a host-declared, framework-side
/// FlatBuffers buffer identified by `schema_id` / `schema_version` /
/// `file_identifier`. The transport layer never interprets these bytes; it only
/// carries them losslessly alongside the generic `Value` snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedProjectionData {
    /// Projection key (host-declared identity of this projection).
    pub key: String,
    /// Stable schema identifier for the typed payload.
    pub schema_id: String,
    /// Schema version of the typed payload. Defaults to `1` on the wire.
    pub schema_version: u32,
    /// FlatBuffers file identifier of the typed payload, if any.
    pub file_identifier: String,
    /// Opaque typed payload bytes, carried verbatim by the transport.
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateFrameDecodeError {
    InvalidFlatbuffer(String),
    InvalidValue(String),
    MissingSnapshotPayload,
    MissingPanicPayload,
    UnexpectedPanicFrame(String),
}

impl fmt::Display for UpdateFrameDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFlatbuffer(msg) => write!(f, "invalid update frame: {msg}"),
            Self::InvalidValue(msg) => write!(f, "invalid update value: {msg}"),
            Self::MissingSnapshotPayload => write!(f, "snapshot frame missing payload"),
            Self::MissingPanicPayload => write!(f, "panic frame missing payload"),
            Self::UnexpectedPanicFrame(msg) => write!(f, "expected snapshot, got panic: {msg}"),
        }
    }
}

impl std::error::Error for UpdateFrameDecodeError {}

/// Encode a full snapshot payload as one FlatBuffers update frame.
///
/// Backward-compatible: equivalent to [`encode_snapshot_with_typed`] with an
/// empty typed-projection sidecar. Because no `typed_projections` slot is
/// written, the wire bytes are byte-identical to the pre-sidecar format.
#[must_use]
pub fn encode_snapshot_value(snapshot: Value) -> UpdateFrameBytes {
    encode_snapshot_with_typed(snapshot, &[])
}

/// Encode a snapshot with an optional typed projection sidecar.
///
/// When `typed` is empty, the result is byte-identical to
/// [`encode_snapshot_value`] (the optional `typed_projections` vector is never
/// added to the FlatBuffers table, so no new vtable slot appears). Each entry's
/// `payload` is carried verbatim as opaque `[ubyte]`; the transport layer never
/// interprets typed payload bytes.
#[must_use]
pub fn encode_snapshot_with_typed(
    snapshot: Value,
    typed: &[TypedProjectionData],
) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let payload = encode_value(&mut builder, &snapshot);
    let typed_projections = encode_typed_projections(&mut builder, typed);
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            payload: Some(payload),
            typed_projections,
        },
    );
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Build the `typed_projections` vector, returning `None` when there are no
/// entries so the optional FlatBuffers slot is omitted entirely (wire-stable).
fn encode_typed_projections<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    typed: &[TypedProjectionData],
) -> Option<
    WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::TypedProjection<'bldr>>>>,
> {
    if typed.is_empty() {
        return None;
    }
    let offsets: Vec<_> = typed
        .iter()
        .map(|entry| {
            let schema_id = builder.create_string(&entry.schema_id);
            let file_identifier = builder.create_string(&entry.file_identifier);
            let payload = builder.create_vector(&entry.payload);
            let typed_payload = fb::TypedPayload::create(
                builder,
                &fb::TypedPayloadArgs {
                    schema_id: Some(schema_id),
                    schema_version: entry.schema_version,
                    file_identifier: Some(file_identifier),
                    payload: Some(payload),
                },
            );
            let key = builder.create_string(&entry.key);
            fb::TypedProjection::create(
                builder,
                &fb::TypedProjectionArgs {
                    key: Some(key),
                    payload: Some(typed_payload),
                },
            )
        })
        .collect();
    Some(builder.create_vector(&offsets))
}

/// Encode the terminal actor-death signal as one FlatBuffers update frame.
#[must_use]
pub fn encode_panic(msg: impl Into<String>) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let msg = builder.create_string(&msg.into());
    let panic = fb::PanicFrame::create(&mut builder, &fb::PanicFrameArgs { msg: Some(msg) });
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Panic,
            snapshot: None,
            panic: Some(panic),
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

pub fn decode_update_frame(bytes: &[u8]) -> Result<UpdateEnvelope, UpdateFrameDecodeError> {
    if !fb::update_frame_buffer_has_identifier(bytes) {
        return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
            "missing NMPU file identifier".to_string(),
        ));
    }
    let frame = fb::root_as_update_frame(bytes)
        .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    match frame.kind() {
        kind if kind == fb::FrameKind::Snapshot => {
            let snapshot = frame
                .snapshot()
                .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
            let payload = snapshot
                .payload()
                .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
            Ok(UpdateEnvelope::Snapshot(decode_value(payload)?))
        }
        kind if kind == fb::FrameKind::Panic => {
            let panic = frame
                .panic()
                .ok_or(UpdateFrameDecodeError::MissingPanicPayload)?;
            Ok(UpdateEnvelope::Panic(PanicFrame {
                msg: panic.msg().to_string(),
            }))
        }
        other => Err(UpdateFrameDecodeError::InvalidFlatbuffer(format!(
            "unknown frame kind {}",
            other.0
        ))),
    }
}

pub fn decode_snapshot_payload(bytes: &[u8]) -> Result<Value, UpdateFrameDecodeError> {
    match decode_update_frame(bytes)? {
        UpdateEnvelope::Snapshot(value) => Ok(value),
        UpdateEnvelope::Panic(panic) => {
            Err(UpdateFrameDecodeError::UnexpectedPanicFrame(panic.msg))
        }
    }
}

/// Decode a snapshot frame, returning both the generic `Value` payload and the
/// typed projection sidecar (as opaque [`TypedProjectionData`] entries).
///
/// Frames produced before the sidecar existed — or by
/// [`encode_snapshot_value`] — decode with an empty typed vector, so this is a
/// strict superset of [`decode_snapshot_payload`]. The typed payload bytes are
/// returned verbatim; `nmp-core` never interprets them.
pub fn decode_snapshot_with_typed(
    bytes: &[u8],
) -> Result<(Value, Vec<TypedProjectionData>), UpdateFrameDecodeError> {
    if !fb::update_frame_buffer_has_identifier(bytes) {
        return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
            "missing NMPU file identifier".to_string(),
        ));
    }
    let frame = fb::root_as_update_frame(bytes)
        .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    match frame.kind() {
        kind if kind == fb::FrameKind::Snapshot => {
            let snapshot = frame
                .snapshot()
                .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
            let payload = snapshot
                .payload()
                .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
            let value = decode_value(payload)?;
            let typed = decode_typed_projections(&snapshot)?;
            Ok((value, typed))
        }
        kind if kind == fb::FrameKind::Panic => {
            let panic = frame
                .panic()
                .ok_or(UpdateFrameDecodeError::MissingPanicPayload)?;
            Err(UpdateFrameDecodeError::UnexpectedPanicFrame(
                panic.msg().to_string(),
            ))
        }
        other => Err(UpdateFrameDecodeError::InvalidFlatbuffer(format!(
            "unknown frame kind {}",
            other.0
        ))),
    }
}

fn decode_typed_projections(
    snapshot: &fb::SnapshotFrame<'_>,
) -> Result<Vec<TypedProjectionData>, UpdateFrameDecodeError> {
    let Some(projections) = snapshot.typed_projections() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(projections.len());
    for index in 0..projections.len() {
        let projection = projections.get(index);
        let key = projection
            .key()
            .ok_or_else(|| {
                UpdateFrameDecodeError::InvalidValue(format!(
                    "typed projection at index {index} missing key"
                ))
            })?
            .to_string();
        let typed = projection.payload().ok_or_else(|| {
            UpdateFrameDecodeError::InvalidValue(format!(
                "typed projection {key:?} missing payload"
            ))
        })?;
        let payload = typed
            .payload()
            .map(|bytes| bytes.bytes().to_vec())
            .unwrap_or_default();
        out.push(TypedProjectionData {
            key,
            schema_id: typed.schema_id().unwrap_or_default().to_string(),
            schema_version: typed.schema_version(),
            file_identifier: typed.file_identifier().unwrap_or_default().to_string(),
            payload,
        });
    }
    Ok(out)
}

fn encode_value<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    value: &Value,
) -> WIPOffset<fb::Value<'bldr>> {
    match value {
        Value::Null => fb::Value::create(
            builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::Null,
                ..Default::default()
            },
        ),
        Value::Bool(v) => fb::Value::create(
            builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::Bool,
                bool_value: *v,
                ..Default::default()
            },
        ),
        Value::Number(v) => encode_number(builder, v),
        Value::String(v) => {
            let string = builder.create_string(v);
            fb::Value::create(
                builder,
                &fb::ValueArgs {
                    kind: fb::ValueKind::String,
                    string_value: Some(string),
                    ..Default::default()
                },
            )
        }
        Value::Array(values) => {
            let offsets: Vec<_> = values
                .iter()
                .map(|value| encode_value(builder, value))
                .collect();
            let list = builder.create_vector(&offsets);
            fb::Value::create(
                builder,
                &fb::ValueArgs {
                    kind: fb::ValueKind::List,
                    list: Some(list),
                    ..Default::default()
                },
            )
        }
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let offsets: Vec<_> = entries
                .iter()
                .map(|(key, value)| {
                    let key = builder.create_string(key);
                    let value = encode_value(builder, value);
                    fb::Pair::create(
                        builder,
                        &fb::PairArgs {
                            key: Some(key),
                            value: Some(value),
                        },
                    )
                })
                .collect();
            let map = builder.create_vector(&offsets);
            fb::Value::create(
                builder,
                &fb::ValueArgs {
                    kind: fb::ValueKind::Map,
                    map: Some(map),
                    ..Default::default()
                },
            )
        }
    }
}

fn encode_number<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    value: &Number,
) -> WIPOffset<fb::Value<'bldr>> {
    if let Some(v) = value.as_i64() {
        fb::Value::create(
            builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::Int,
                int_value: v,
                ..Default::default()
            },
        )
    } else if let Some(v) = value.as_u64() {
        fb::Value::create(
            builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::UInt,
                uint_value: v,
                ..Default::default()
            },
        )
    } else {
        fb::Value::create(
            builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::Float,
                float_value: value.as_f64().unwrap_or_default(),
                ..Default::default()
            },
        )
    }
}

fn decode_value(value: fb::Value<'_>) -> Result<Value, UpdateFrameDecodeError> {
    match value.kind() {
        kind if kind == fb::ValueKind::Null => Ok(Value::Null),
        kind if kind == fb::ValueKind::Bool => Ok(Value::Bool(value.bool_value())),
        kind if kind == fb::ValueKind::Int => Ok(Value::Number(Number::from(value.int_value()))),
        kind if kind == fb::ValueKind::UInt => Ok(Value::Number(Number::from(value.uint_value()))),
        kind if kind == fb::ValueKind::Float => {
            let float = value.float_value();
            if !float.is_finite() {
                return Err(UpdateFrameDecodeError::InvalidValue(
                    "non-finite float value".to_string(),
                ));
            }
            Number::from_f64(float)
                .map(Value::Number)
                .ok_or_else(|| UpdateFrameDecodeError::InvalidValue("invalid float".to_string()))
        }
        kind if kind == fb::ValueKind::String => {
            let string = value.string_value().ok_or_else(|| {
                UpdateFrameDecodeError::InvalidValue(
                    "string value missing string_value".to_string(),
                )
            })?;
            Ok(Value::String(string.to_string()))
        }
        kind if kind == fb::ValueKind::List => {
            let list = value.list().ok_or_else(|| {
                UpdateFrameDecodeError::InvalidValue("list value missing list".to_string())
            })?;
            let values = (0..list.len())
                .map(|index| decode_value(list.get(index)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(values))
        }
        kind if kind == fb::ValueKind::Map => {
            let mut values = Map::new();
            let map = value.map().ok_or_else(|| {
                UpdateFrameDecodeError::InvalidValue("map value missing map".to_string())
            })?;
            for index in 0..map.len() {
                let pair = map.get(index);
                let value = pair.value().ok_or_else(|| {
                    UpdateFrameDecodeError::InvalidValue(format!(
                        "map pair at index {index} missing value"
                    ))
                })?;
                values.insert(pair.key().to_string(), decode_value(value)?);
            }
            Ok(Value::Object(values))
        }
        other => Err(UpdateFrameDecodeError::InvalidValue(format!(
            "unknown value kind {}",
            other.0
        ))),
    }
}

/// Best-effort message extraction from a `catch_unwind` payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic in actor thread".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden_snapshot_payload() -> Value {
        serde_json::json!({
            "schema_version": SNAPSHOT_SCHEMA_VERSION,
            "rev": 42,
            "running": true,
            "projections": { "timeline": [{ "id": "a", "score": 1.5 }] }
        })
    }

    fn decode_hex_fixture(input: &str) -> Vec<u8> {
        let compact: String = input.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert_eq!(compact.len() % 2, 0, "hex fixture must contain full bytes");
        compact
            .as_bytes()
            .chunks(2)
            .map(|pair| {
                let hex = std::str::from_utf8(pair).expect("fixture is ascii hex");
                u8::from_str_radix(hex, 16).expect("fixture is valid hex")
            })
            .collect()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn snapshot_frame_has_flatbuffer_identifier_and_round_trips() {
        let payload = golden_snapshot_payload();
        let wire = encode_snapshot_value(payload.clone());
        assert!(fb::update_frame_buffer_has_identifier(&wire));
        assert_eq!(decode_snapshot_payload(&wire).expect("decode"), payload);
    }

    #[test]
    fn empty_typed_sidecar_is_byte_identical_to_legacy_encoder() {
        let payload = golden_snapshot_payload();
        let legacy = encode_snapshot_value(payload.clone());
        let with_empty = encode_snapshot_with_typed(payload, &[]);
        assert_eq!(
            legacy, with_empty,
            "an empty typed sidecar must not change the wire bytes"
        );
    }

    #[test]
    fn typed_sidecar_round_trips_opaque_payloads_alongside_value() {
        let payload = golden_snapshot_payload();
        let typed = vec![
            TypedProjectionData {
                key: "timeline".to_string(),
                schema_id: "nmp.timeline".to_string(),
                schema_version: 3,
                file_identifier: "TMLN".to_string(),
                payload: vec![0x00, 0x01, 0xfe, 0xff, 0x42],
            },
            TypedProjectionData {
                key: "contacts".to_string(),
                schema_id: "nmp.contacts".to_string(),
                schema_version: 1,
                file_identifier: String::new(),
                payload: Vec::new(),
            },
        ];

        let wire = encode_snapshot_with_typed(payload.clone(), &typed);
        assert!(fb::update_frame_buffer_has_identifier(&wire));

        let (decoded_value, decoded_typed) =
            decode_snapshot_with_typed(&wire).expect("decode with typed");
        assert_eq!(decoded_value, payload, "generic value must survive");
        assert_eq!(decoded_typed, typed, "typed sidecar must survive verbatim");

        // The generic-only decoder must still see the same Value, ignoring the
        // typed sidecar entirely.
        assert_eq!(
            decode_snapshot_payload(&wire).expect("legacy decode"),
            payload
        );
    }

    #[test]
    fn legacy_frame_decodes_with_empty_typed_sidecar() {
        let payload = golden_snapshot_payload();
        let wire = encode_snapshot_value(payload.clone());
        let (decoded_value, decoded_typed) =
            decode_snapshot_with_typed(&wire).expect("decode legacy frame");
        assert_eq!(decoded_value, payload);
        assert!(
            decoded_typed.is_empty(),
            "a frame without the sidecar must decode to zero typed projections"
        );
    }

    #[test]
    fn decode_snapshot_with_typed_rejects_panic_frame() {
        let wire = encode_panic("boom");
        let err = decode_snapshot_with_typed(&wire).expect_err("panic must not decode as snapshot");
        assert!(matches!(
            err,
            UpdateFrameDecodeError::UnexpectedPanicFrame(_)
        ));
    }

    #[test]
    fn snapshot_v1_wire_fixture_is_stable() {
        let wire = encode_snapshot_value(golden_snapshot_payload());
        let expected = decode_hex_fixture(include_str!(
            "../tests/fixtures/update_frame_snapshot_v1.fb.hex"
        ));
        if wire != expected {
            eprintln!("actual snapshot_v1 fixture hex:\n{}", encode_hex(&wire));
        }
        assert_eq!(
            wire, expected,
            "snapshot v1 FlatBuffers wire fixture drifted"
        );
    }

    #[test]
    fn non_finite_float_fails_decode_instead_of_degrading_to_null() {
        let mut builder = FlatBufferBuilder::new();
        let payload = fb::Value::create(
            &mut builder,
            &fb::ValueArgs {
                kind: fb::ValueKind::Float,
                float_value: f64::NAN,
                ..Default::default()
            },
        );
        let snapshot = fb::SnapshotFrame::create(
            &mut builder,
            &fb::SnapshotFrameArgs {
                schema_version: SNAPSHOT_SCHEMA_VERSION,
                payload: Some(payload),
                typed_projections: None,
            },
        );
        let root = fb::UpdateFrame::create(
            &mut builder,
            &fb::UpdateFrameArgs {
                kind: fb::FrameKind::Snapshot,
                snapshot: Some(snapshot),
                panic: None,
            },
        );
        fb::finish_update_frame_buffer(&mut builder, root);

        let err = decode_snapshot_payload(builder.finished_data()).expect_err("must reject NaN");
        assert!(matches!(err, UpdateFrameDecodeError::InvalidValue(_)));
    }

    #[test]
    fn panic_frame_round_trips() {
        let wire = encode_panic(r#"actor "panicked" \ boom"#);
        assert!(fb::update_frame_buffer_has_identifier(&wire));
        match decode_update_frame(&wire).expect("decode") {
            UpdateEnvelope::Panic(panic) => assert_eq!(panic.msg, r#"actor "panicked" \ boom"#),
            other => panic!("expected panic frame, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_schema_version_is_one() {
        assert_eq!(SNAPSHOT_SCHEMA_VERSION, 1);
    }

    #[test]
    fn panic_message_extracts_string_and_str_payloads() {
        let from_string = std::panic::catch_unwind(|| panic!("{}", "owned panic".to_string()))
            .expect_err("must unwind");
        assert_eq!(panic_message(&*from_string), "owned panic");

        let from_str =
            std::panic::catch_unwind(|| panic!("static str panic")).expect_err("must unwind");
        assert_eq!(panic_message(&*from_str), "static str panic");
    }

    #[test]
    fn panic_message_degrades_non_string_payload() {
        let payload =
            std::panic::catch_unwind(|| std::panic::panic_any(42u32)).expect_err("must unwind");
        assert_eq!(panic_message(&*payload), "unknown panic in actor thread");
    }

    #[test]
    fn actor_death_emits_decodable_panic_frame_on_channel() {
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<UpdateFrameBytes>();
        let supervisor_tx = tx.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(tx);
            panic!("kernel loop exploded");
        }));

        if let Err(e) = result {
            let msg = panic_message(&*e);
            let frame = encode_panic(format!("actor thread died: {msg}"));
            let _ = supervisor_tx.send(frame);
        }
        drop(supervisor_tx);

        let frame = rx.recv().expect("panic frame must reach the host");
        match decode_update_frame(&frame).expect("frame decodes") {
            UpdateEnvelope::Panic(p) => {
                assert!(p.msg.contains("actor thread died"));
                assert!(p.msg.contains("kernel loop exploded"));
            }
            other => panic!("expected Panic frame, got {other:?}"),
        }
        assert!(rx.recv().is_err(), "channel must close after panic frame");
    }
}
