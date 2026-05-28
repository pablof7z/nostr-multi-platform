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
        "../../tests/fixtures/update_frame_snapshot_v1.fb.hex"
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
