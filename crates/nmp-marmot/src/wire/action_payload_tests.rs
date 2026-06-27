//! Round-trip + fail-closed tests for the `nmp.marmot` action-payload codec
//! (ADR-0064 / #2169, M14-1c). Every arm of the `MarmotAction` union is
//! covered, including optional-field variants (`None` vs `Some`, empty vs
//! non-empty vec). Fail-closed gates: wrong identifier, wrong schema_version,
//! junk bytes, and wrong-namespace twin.

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use super::generated::nmp::marmot as fb;
use super::SCHEMA_VERSION;
use crate::projection::action::MarmotAction;

// ── round-trip helpers ────────────────────────────────────────────────────────

fn rt(action: &MarmotAction) -> MarmotAction {
    MarmotAction::decode(&action.encode()).expect("should decode")
}

// ── PublishKeyPackage ─────────────────────────────────────────────────────────

#[test]
fn publish_key_package_no_relays_round_trips() {
    let a = MarmotAction::PublishKeyPackage { relays: vec![] };
    assert_eq!(rt(&a), a);
}

#[test]
fn publish_key_package_with_relays_round_trips() {
    let a = MarmotAction::PublishKeyPackage {
        relays: vec![
            "wss://relay.example".to_string(),
            "wss://relay2.example".to_string(),
        ],
    };
    assert_eq!(rt(&a), a);
}

// ── CreateGroup ───────────────────────────────────────────────────────────────

#[test]
fn create_group_minimal_round_trips() {
    let a = MarmotAction::CreateGroup {
        name: "Engineering".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: None,
        signed_key_package_events_json: vec![],
        relays: vec![],
    };
    assert_eq!(rt(&a), a);
}

#[test]
fn create_group_full_round_trips() {
    let a = MarmotAction::CreateGroup {
        name: "Engineering".to_string(),
        description: "Team chat".to_string(),
        invitee_text: Some("npub1abc npub1def".to_string()),
        invitee_npubs: Some(vec!["npub1abc".to_string(), "npub1def".to_string()]),
        signed_key_package_events_json: vec![
            serde_json::json!({"id":"abc","sig":"xyz"}),
        ],
        relays: vec!["wss://relay.example".to_string()],
    };
    assert_eq!(rt(&a), a);
}

#[test]
fn create_group_invitee_npubs_none_vs_some_empty_distinguished() {
    // None and Some([]) must round-trip distinctly.
    let with_none = MarmotAction::CreateGroup {
        name: "A".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: None,
        signed_key_package_events_json: vec![],
        relays: vec![],
    };
    let with_some_empty = MarmotAction::CreateGroup {
        name: "A".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: Some(vec![]),
        signed_key_package_events_json: vec![],
        relays: vec![],
    };
    let decoded_none = rt(&with_none);
    let decoded_some = rt(&with_some_empty);
    assert_eq!(decoded_none, with_none, "None must round-trip as None");
    assert_eq!(decoded_some, with_some_empty, "Some([]) must round-trip as Some([])");
    assert_ne!(decoded_none, decoded_some, "None != Some([])");
}

// ── Invite ────────────────────────────────────────────────────────────────────

#[test]
fn invite_minimal_round_trips() {
    let a = MarmotAction::Invite {
        group_id_hex: "deadbeef1234".to_string(),
        invitee_text: None,
        invitee_npubs: None,
        signed_key_package_events_json: vec![],
    };
    assert_eq!(rt(&a), a);
}

#[test]
fn invite_full_round_trips() {
    let a = MarmotAction::Invite {
        group_id_hex: "deadbeef1234".to_string(),
        invitee_text: Some("npub1abc".to_string()),
        invitee_npubs: Some(vec!["npub1abc".to_string()]),
        signed_key_package_events_json: vec![serde_json::json!({"id":"abc"})],
    };
    assert_eq!(rt(&a), a);
}

#[test]
fn invite_invitee_npubs_none_vs_some_empty_distinguished() {
    // Invite, like CreateGroup, must distinguish `None` from `Some([])` for
    // `invitee_npubs` across a round trip (#2169 codex SHOULD-FIX 3).
    let with_none = MarmotAction::Invite {
        group_id_hex: "deadbeef1234".to_string(),
        invitee_text: None,
        invitee_npubs: None,
        signed_key_package_events_json: vec![],
    };
    let with_some_empty = MarmotAction::Invite {
        group_id_hex: "deadbeef1234".to_string(),
        invitee_text: None,
        invitee_npubs: Some(vec![]),
        signed_key_package_events_json: vec![],
    };
    let decoded_none = rt(&with_none);
    let decoded_some = rt(&with_some_empty);
    assert_eq!(decoded_none, with_none, "None must round-trip as None");
    assert_eq!(
        decoded_some, with_some_empty,
        "Some([]) must round-trip as Some([])"
    );
    assert_ne!(decoded_none, decoded_some, "None != Some([])");
}

// ── Send ──────────────────────────────────────────────────────────────────────

#[test]
fn send_round_trips() {
    let a = MarmotAction::Send {
        group_id_hex: "deadbeef1234".to_string(),
        text: "Hello MLS world!".to_string(),
    };
    assert_eq!(rt(&a), a);
}

// ── Leave ─────────────────────────────────────────────────────────────────────

#[test]
fn leave_round_trips() {
    let a = MarmotAction::Leave { group_id_hex: "deadbeef1234".to_string() };
    assert_eq!(rt(&a), a);
}

// ── Remove ────────────────────────────────────────────────────────────────────

#[test]
fn remove_no_members_round_trips() {
    let a = MarmotAction::Remove {
        group_id_hex: "deadbeef1234".to_string(),
        member_npubs: vec![],
    };
    assert_eq!(rt(&a), a);
}

#[test]
fn remove_with_members_round_trips() {
    let a = MarmotAction::Remove {
        group_id_hex: "deadbeef1234".to_string(),
        member_npubs: vec!["npub1alice".to_string(), "npub1bob".to_string()],
    };
    assert_eq!(rt(&a), a);
}

// ── AcceptWelcome ─────────────────────────────────────────────────────────────

#[test]
fn accept_welcome_round_trips() {
    let a = MarmotAction::AcceptWelcome { welcome_id_hex: "cafebabe5678".to_string() };
    assert_eq!(rt(&a), a);
}

// ── DeclineWelcome ────────────────────────────────────────────────────────────

#[test]
fn decline_welcome_round_trips() {
    let a = MarmotAction::DeclineWelcome { welcome_id_hex: "cafebabe5678".to_string() };
    assert_eq!(rt(&a), a);
}

// ── ClearPending ─────────────────────────────────────────────────────────────

#[test]
fn clear_pending_round_trips() {
    let a = MarmotAction::ClearPending { group_id_hex: "deadbeef1234".to_string() };
    assert_eq!(rt(&a), a);
}

// ── golden NMMA byte fixtures (cross-language byte parity — #2169) ─────────────
//
// These canonical byte fixtures are the SINGLE SOURCE OF TRUTH that the Rust
// encoder AND the generated Swift/Kotlin host builders must each reproduce
// byte-for-byte. Two representative arms are pinned: an EMPTY-vector arm
// (`PublishKeyPackage { relays: [] }` — proves the present-empty non-optional
// vector encoding) and a POPULATED arm (`CreateGroup` with strings + present
// `invitee_npubs` + present-empty json vec + populated `relays`).
//
// Each is pinned at TWO levels:
//   * PAYLOAD (NMMA): exactly what `MarmotAction::encode()` returns.
//   * ENVELOPE (NMPD): the full `DispatchEnvelope` for correlation id
//     `"golden-corr"` — exactly what the generated `GeneratedActionBuilders`
//     host method returns. The host builders emit the WHOLE envelope, so the
//     Swift/Kotlin golden tests do a direct byte compare against the ENVELOPE
//     hex (no host-side FlatBuffers decoder needed). The same hex is embedded in:
//       - Kotlin: apps/chirp/android/app/src/test/resources/fixtures/
//                 marmot_publish_key_package_empty_golden_v1.fb.hex
//                 marmot_create_group_populated_golden_v1.fb.hex
//                 (+ MarmotBuilderGoldenTest.kt)
//       - Swift:  apps/chirp/ios/ChirpTests/MarmotBuilderGoldenTests.swift
//
// This FORCES Rust-encode ↔ host-builder byte parity and blesses the
// present-empty non-optional vector encoding. The envelope creation order
// (correlation string, namespace string, payload vector) is identical in Rust
// `encode_dispatch_envelope` and the generated `encodeDispatchEnvelope`, so the
// envelope bytes are byte-identical too.
//
// To regenerate after an intentional schema/encoder change: add a temporary
// dump test that prints `to_hex(&action.encode())` and the envelope, run it,
// then paste the new hex here AND into the Swift/Kotlin fixtures.

/// `PublishKeyPackage { relays: [] }` PAYLOAD — the EMPTY-vector arm. `relays`
/// is a NON-OPTIONAL `[string]` emitted as a PRESENT empty vector (not absent).
const GOLDEN_PUBLISH_KEY_PACKAGE_EMPTY: &str = "140000004e4d4d4100000a001200080007000c000a00000000000001010000000c0000000000060008000400060000000400000000000000";

/// `CreateGroup { .. }` PAYLOAD — the POPULATED arm.
const GOLDEN_CREATE_GROUP_POPULATED: &str = "140000004e4d4d4100000a001000080007000c000a00000000000002010000001400000010001c00040008000c0010001400180010000000180000002400000030000000440000006c0000006c0000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c6500";

/// The EMPTY-vector arm wrapped in a `DispatchEnvelope` (correlation
/// `"golden-corr"`, namespace `nmp.marmot`) — what the Swift/Kotlin
/// `marmotPublishKeyPackage(correlationId: "golden-corr")` builder returns.
const GOLDEN_ENVELOPE_PUBLISH_KEY_PACKAGE_EMPTY: &str = "140000004e4d50440c001400040008000c0010000c0000005c00000048000000010000000400000038000000140000004e4d4d4100000a001200080007000c000a00000000000001010000000c00000000000600080004000600000004000000000000000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200";

/// The POPULATED arm wrapped in a `DispatchEnvelope` — what the Swift/Kotlin
/// `marmotCreateGroup(correlationId: "golden-corr", ..)` builder returns.
const GOLDEN_ENVELOPE_CREATE_GROUP_POPULATED: &str = "140000004e4d50440c001400040008000c0010000c000000fc000000e80000000100000004000000d8000000140000004e4d4d4100000a001000080007000c000a00000000000002010000001400000010001c00040008000c0010001400180010000000180000002400000030000000440000006c0000006c0000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c65000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200";

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn golden_envelope(action: &MarmotAction) -> Vec<u8> {
    use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
    encode_dispatch_envelope(
        "golden-corr",
        "nmp.marmot",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    )
}

#[test]
fn golden_publish_key_package_empty_payload_byte_identical() {
    let action = MarmotAction::PublishKeyPackage { relays: vec![] };
    assert_eq!(
        to_hex(&action.encode()),
        GOLDEN_PUBLISH_KEY_PACKAGE_EMPTY,
        "Rust PAYLOAD encode must equal the canonical golden NMMA bytes \
         (Swift/Kotlin builders assert the SAME envelope hex — keep all in sync)"
    );
    assert_eq!(
        to_hex(&golden_envelope(&action)),
        GOLDEN_ENVELOPE_PUBLISH_KEY_PACKAGE_EMPTY,
        "Rust ENVELOPE must equal the canonical golden NMPD bytes that the host \
         builder marmotPublishKeyPackage(correlationId: \"golden-corr\") returns"
    );
}

#[test]
fn golden_create_group_populated_payload_byte_identical() {
    let action = MarmotAction::CreateGroup {
        name: "Engineering".to_string(),
        description: "Team chat".to_string(),
        invitee_text: Some("npub1abc npub1def".to_string()),
        invitee_npubs: Some(vec!["npub1abc".to_string(), "npub1def".to_string()]),
        signed_key_package_events_json: vec![],
        relays: vec!["wss://relay.example".to_string()],
    };
    assert_eq!(
        to_hex(&action.encode()),
        GOLDEN_CREATE_GROUP_POPULATED,
        "Rust PAYLOAD encode must equal the canonical golden NMMA bytes \
         (Swift/Kotlin builders assert the SAME envelope hex — keep all in sync)"
    );
    assert_eq!(
        to_hex(&golden_envelope(&action)),
        GOLDEN_ENVELOPE_CREATE_GROUP_POPULATED,
        "Rust ENVELOPE must equal the canonical golden NMPD bytes that the host \
         builder marmotCreateGroup(correlationId: \"golden-corr\", ..) returns"
    );
}

// ── fail-closed gates ─────────────────────────────────────────────────────────

#[test]
fn malformed_buffers_rejected() {
    assert!(matches!(
        MarmotAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        MarmotAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_identifier_rejected() {
    let a = MarmotAction::Send {
        group_id_hex: "deadbeef1234".to_string(),
        text: "hi".to_string(),
    };
    let mut bytes = a.encode();
    // File identifier lives at bytes[4..8].
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        MarmotAction::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_schema_version_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let gid = fbb.create_string("deadbeef1234");
    let text = fbb.create_string("hello");
    let body = fb::Send::create(&mut fbb, &fb::SendArgs {
        group_id_hex: Some(gid),
        text: Some(text),
    });
    let root = fb::MarmotActionPayload::create(
        &mut fbb,
        &fb::MarmotActionPayloadArgs {
            schema_version: 999,
            body_type: fb::MarmotActionBody::Send,
            body: Some(body.as_union_value()),
        },
    );
    fb::finish_marmot_action_payload_buffer(&mut fbb, root);
    let err = MarmotAction::decode(fbb.finished_data()).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn unknown_discriminant_rejected() {
    // Manually craft a root with an unknown union discriminant (255).
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let gid = fbb.create_string("deadbeef1234");
    // Use Leave as a placeholder body (we'll patch the discriminant byte).
    let body = fb::Leave::create(&mut fbb, &fb::LeaveArgs { group_id_hex: Some(gid) });
    let root = fb::MarmotActionPayload::create(
        &mut fbb,
        &fb::MarmotActionPayloadArgs {
            schema_version: SCHEMA_VERSION,
            body_type: fb::MarmotActionBody(255),
            body: Some(body.as_union_value()),
        },
    );
    fb::finish_marmot_action_payload_buffer(&mut fbb, root);
    assert!(matches!(
        MarmotAction::decode(fbb.finished_data()),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
