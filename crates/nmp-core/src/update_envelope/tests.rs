use super::*;

/// Drift assertion for `update_frame_tier3_golden_v1.fb.hex`.
///
/// Encodes `golden_envelope()` and asserts the bytes are identical to the
/// checked-in fixture.  Any change to the Tier-3 schema (`nmp_update.fbs`),
/// the Rust encoder, or `golden_envelope()` will cause this test to **fail**
/// with the new hex printed to stdout — forcing an explicit, reviewed fixture
/// regeneration in BOTH the Rust tree (`crates/nmp-core/tests/fixtures/`) and
/// the Android tree in the external Chirp repo (`../chirp/android/app/src/test/resources/fixtures/`).
///
/// To regenerate after an intentional schema change:
///   1. Run this test with `--nocapture` and copy the printed hex line into
///      both fixture files.
///   2. Re-run to confirm the assertion passes.
#[test]
fn tier3_golden_fixture_matches_encoder() {
    let wire = encode_snapshot_frame(&golden_envelope(), &[]);
    let expected = decode_hex_fixture(include_str!(
        "../../tests/fixtures/update_frame_tier3_golden_v1.fb.hex"
    ));
    if wire != expected {
        let actual_hex: String = wire.iter().map(|b| format!("{b:02x}")).collect();
        eprintln!(
            "\ntier3 golden fixture drifted — new hex (update BOTH fixture files):\n{actual_hex}"
        );
    }
    assert_eq!(
        wire, expected,
        "update_frame_tier3_golden_v1.fb.hex drifted from the encoder — regenerate both \
         crates/nmp-core/tests/fixtures/ and the external Chirp repo's \
         ../chirp/android/app/src/test/resources/fixtures/ copies"
    );
    // Golden sanity: the frame must carry the NMPU identifier.
    assert!(fb::update_frame_buffer_has_identifier(&wire));
    // Round-trip: the decoded envelope must equal golden_envelope().
    assert_eq!(
        decode_snapshot_envelope(&wire).expect("decode"),
        golden_envelope()
    );
}

/// A representative typed envelope for round-trip tests. Fields already covered
/// by the checked-in golden fixture carry the fixture values; metrics that were
/// historically omitted from the auxiliary encoder stay zero here so the
/// fixture remains stable and get explicit coverage below.
fn golden_envelope() -> SnapshotEnvelope {
    SnapshotEnvelope {
        rev: 42,
        kernel_schema_version: 1,
        last_tick_ms: 1_700_000_123_456,
        running: true,
        update_kind: "ViewBatch".to_string(),
        note_events: 0,
        events_rx: 7,
        visible_items: 3,
        events_since_last_update: 0,
        actor_queue_depth: 2,
        update_sequence: 11,
        relay_statuses: vec![RelayStatusEntry {
            role: "both".to_string(),
            relay_url: "wss://relay.example".to_string(),
            connection: "connected".to_string(),
            auth: "accepted".to_string(),
            events_rx: 7,
            denied: false,
        }],
        relay_status: Some(RelayStatusEntry {
            role: "aggregate".to_string(),
            relay_url: String::new(),
            connection: "connected".to_string(),
            auth: String::new(),
            events_rx: 7,
            denied: false,
        }),
        wire_subscriptions: vec![WireSubscriptionEntry {
            wire_id: "sub-1".to_string(),
            relay_url: "wss://relay.example".to_string(),
            state: "open".to_string(),
        }],
        last_error_toast: Some("boom".to_string()),
        last_error_category: Some("publish".to_string()),
        last_planner_error: None,
        // ADR-0070 Rung 2: non-default values to catch codec transpositions.
        snapshot_epoch: 3,
        session_id: 1_700_000_000_000,
        // ADR-0070 Rung 3 S5: serialize_us is produced by the production kernel
        // path (encode_snapshot_with_envelope) which reads self.last_serialize_us.
        // The auxiliary encode_snapshot_frame path writes 0 via MetricsArgs default,
        // so the round-trip of this fixture reads back 0.
        serialize_us: 0,
        external_event_sink_channel_overflow_drops: 0,
    }
}

#[test]
fn extended_metrics_round_trip() {
    let envelope = SnapshotEnvelope {
        note_events: 5,
        events_since_last_update: 2,
        ..golden_envelope()
    };
    let wire = encode_snapshot_frame(&envelope, &[]);

    assert_eq!(
        decode_snapshot_envelope(&wire).expect("decode"),
        envelope,
        "note_events and events_since_last_update must survive encode/decode"
    );
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

#[test]
fn snapshot_frame_has_flatbuffer_identifier_and_round_trips() {
    let envelope = golden_envelope();
    let wire = encode_snapshot_frame(&envelope, &[]);
    assert!(fb::update_frame_buffer_has_identifier(&wire));
    assert_eq!(
        decode_snapshot_envelope(&wire).expect("decode"),
        envelope,
        "every SnapshotEnvelope field must survive the encode/decode round trip"
    );
}

#[test]
fn decode_update_frame_carries_the_same_envelope_as_decode_snapshot_envelope() {
    let envelope = golden_envelope();
    let wire = encode_snapshot_frame(&envelope, &[]);
    match decode_update_frame(&wire).expect("decode") {
        UpdateEnvelope::Snapshot(decoded) => assert_eq!(
            decoded, envelope,
            "the UpdateEnvelope::Snapshot arm and decode_snapshot_envelope must agree"
        ),
        other => panic!("expected snapshot frame, got {other:?}"),
    }
}

#[test]
fn typed_sidecar_round_trips_opaque_payloads_alongside_envelope() {
    let envelope = golden_envelope();
    let typed = vec![
        TypedProjectionData {
            key: "timeline".to_string(),
            schema_id: "nmp.timeline".to_string(),
            schema_version: 3,
            file_identifier: "TMLN".to_string(),
            payload: vec![0x00, 0x01, 0xfe, 0xff, 0x42],
            // ADR-0070 Rung 2: explicit defaults so assert_eq round-trips correctly.
            ..Default::default()
        },
        TypedProjectionData {
            key: "contacts".to_string(),
            schema_id: "nmp.contacts".to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: Vec::new(),
            ..Default::default()
        },
    ];

    let wire = encode_snapshot_frame(&envelope, &typed);
    assert!(fb::update_frame_buffer_has_identifier(&wire));

    let decoded_typed = decode_snapshot_typed_projections(&wire).expect("decode typed sidecar");
    assert_eq!(decoded_typed, typed, "typed sidecar must survive verbatim");

    // The envelope decoder must still see the same envelope, ignoring the
    // typed sidecar entirely.
    assert_eq!(
        decode_snapshot_envelope(&wire).expect("decode envelope"),
        envelope
    );
}

#[test]
fn frame_without_sidecar_decodes_with_empty_typed_vector() {
    let wire = encode_snapshot_frame(&golden_envelope(), &[]);
    let decoded_typed = decode_snapshot_typed_projections(&wire).expect("decode");
    assert!(
        decoded_typed.is_empty(),
        "a frame without the sidecar must decode to zero typed projections"
    );
}

#[test]
fn decode_snapshot_envelope_rejects_panic_frame() {
    let wire = encode_panic("boom");
    let err = decode_snapshot_envelope(&wire).expect_err("panic must not decode as snapshot");
    assert!(matches!(
        err,
        UpdateFrameDecodeError::MissingSnapshotPayload
    ));
    let err =
        decode_snapshot_typed_projections(&wire).expect_err("panic must not decode as snapshot");
    assert!(matches!(
        err,
        UpdateFrameDecodeError::MissingSnapshotPayload
    ));
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
