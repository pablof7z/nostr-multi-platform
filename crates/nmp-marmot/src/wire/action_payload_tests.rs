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
        signed_key_package_events_json: vec![serde_json::json!({"id":"abc","sig":"xyz"})],
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
    assert_eq!(
        decoded_some, with_some_empty,
        "Some([]) must round-trip as Some([])"
    );
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
    let a = MarmotAction::Leave {
        group_id_hex: "deadbeef1234".to_string(),
    };
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
    let a = MarmotAction::AcceptWelcome {
        welcome_id_hex: "cafebabe5678".to_string(),
    };
    assert_eq!(rt(&a), a);
}

// ── DeclineWelcome ────────────────────────────────────────────────────────────

#[test]
fn decline_welcome_round_trips() {
    let a = MarmotAction::DeclineWelcome {
        welcome_id_hex: "cafebabe5678".to_string(),
    };
    assert_eq!(rt(&a), a);
}

// ── ClearPending ─────────────────────────────────────────────────────────────

#[test]
fn clear_pending_round_trips() {
    let a = MarmotAction::ClearPending {
        group_id_hex: "deadbeef1234".to_string(),
    };
    assert_eq!(rt(&a), a);
}

// ── golden NMMA fixtures + cross-language parity contract (#2169) ──────────────
//
// Two representative arms are pinned: an EMPTY-vector arm
// (`PublishKeyPackage { relays: [] }` — proves the present-empty non-optional
// vector encoding) and a POPULATED arm (`CreateGroup` with strings + present
// `invitee_npubs` + present-empty json vec + populated `relays`).
//
// PARITY CONTRACT — why byte-identity is NOT asserted Rust↔host (Option B):
//
// Three encoders touch these payloads:
//   * Rust `MarmotAction::encode()` — uses the flatc-GENERATED `*::create()`
//     builders, which pack table fields by DESCENDING type size then DESCENDING
//     slot (the flatc convention: `add_payload`, `add_schema_version`,
//     `add_action_namespace`, `add_correlation_id` — reverse of slot order).
//   * Swift `GeneratedActionBuilders.marmot*` — HAND-ROLLED `startTable`/`add`
//     in FORWARD slot order.
//   * Kotlin `GeneratedActionBuilders.marmot*` — the SAME hand-rolled forward
//     order.
//
// Field WRITE order changes where each field's value sits in the table body, so
// flatc-generated-`create()` output is byte-DIFFERENT from the hand-rolled host
// builders — while being SEMANTICALLY IDENTICAL (FlatBuffers decodes by vtable
// slot, which is order-independent). Forcing byte-identity would mean either
// re-implementing flatc's field-packing in the hand-rolled emitters, or
// rewriting the SHARED `nmp_core::encode_dispatch_envelope` (used by every
// action, not just marmot) to forward order — neither is justified: in
// production the HOST encodes the envelope and RUST only DECODES it, so the
// bytes are never byte-compared. The invariant that actually matters is
// SEMANTIC: every encoder must agree on present-vs-absent fields so the decoded
// `MarmotAction` is identical (this is the real #2169 fix — all three emit a
// PRESENT empty vector for non-optional `[string]`).
//
// So the parity is pinned as follows:
//   * Rust  — `MarmotAction::encode()` byte-locked to its OWN canonical payload
//     hex below (regression lock on the Rust present-empty encoding).
//   * Rust  — the ACTUAL host-builder envelope bytes (captured from the Kotlin
//     + Swift CI, which are byte-identical to EACH OTHER) round-trip through
//     `decode_dispatch_envelope` + `MarmotAction::decode` to the EXPECTED
//     action (semantic parity on real host bytes — proves the kernel consumes
//     host builder output, including the present-empty vectors).
//   * Kotlin + Swift — each assert their builder output is byte-identical to
//     the SAME host-canonical envelope fixture (`*_golden_v1.fb.hex` /
//     `MarmotBuilderGoldenTests.swift`), which proves Kotlin↔Swift
//     byte-identity — the meaningful cross-SHELL guarantee.
//
// To regenerate after an intentional schema/encoder change: update the Rust
// `MARMOT_PAYLOAD_*` hex from `MarmotAction::encode()`, rebuild the host
// builders, run the Kotlin/Swift golden tests (they print expected-vs-actual on
// failure), and paste the new host envelope hex into `HOST_ENVELOPE_*` here +
// the `.fb.hex` fixtures + the Swift constants.

/// `PublishKeyPackage { relays: [] }` PAYLOAD — exactly what
/// `MarmotAction::encode()` returns (flatc-generated field order). `relays` is
/// a NON-OPTIONAL `[string]` emitted as a PRESENT empty vector (not absent).
const MARMOT_PAYLOAD_PUBLISH_KEY_PACKAGE_EMPTY: &str = "140000004e4d4d4100000a001200080007000c000a00000000000001010000000c0000000000060008000400060000000400000000000000";

/// `CreateGroup { .. }` PAYLOAD — exactly what `MarmotAction::encode()` returns.
const MARMOT_PAYLOAD_CREATE_GROUP_POPULATED: &str = "140000004e4d4d4100000a001000080007000c000a00000000000002010000001400000010001c00040008000c0010001400180010000000180000002400000030000000440000006c0000006c0000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c6500";

/// The EMPTY-vector arm wrapped in a `DispatchEnvelope` (correlation
/// `"golden-corr"`, namespace `nmp.marmot`) — the BYTE-IDENTICAL output of the
/// Kotlin AND Swift `marmotPublishKeyPackage(correlationId: "golden-corr")`
/// builders (hand-rolled forward field order; differs from the Rust flatc order
/// but decodes identically — see the parity contract above). Mirrors
/// `marmot_publish_key_package_empty_golden_v1.fb.hex` +
/// `MarmotBuilderGoldenTests.swift`.
const HOST_ENVELOPE_PUBLISH_KEY_PACKAGE_EMPTY: &str = "140000004e4d50440c00140010000c00080004000c0000001000000001000000440000005000000038000000140000004e4d4d4100000a0012000c000b0004000a00000014000000000000010100000000000600080004000600000004000000000000000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200";

/// The POPULATED arm wrapped in a `DispatchEnvelope` — the BYTE-IDENTICAL
/// output of the Kotlin AND Swift `marmotCreateGroup(correlationId:
/// "golden-corr", ..)` builders. Mirrors
/// `marmot_create_group_populated_golden_v1.fb.hex` +
/// `MarmotBuilderGoldenTests.swift`.
const HOST_ENVELOPE_CREATE_GROUP_POPULATED: &str = "140000004e4d50440c00140010000c00080004000c0000001000000001000000e4000000f0000000d8000000140000004e4d4d4100000a0010000c000b0004000a0000001c000000000000020100000010001c001800140010000c0008000400100000008000000078000000480000002c00000018000000040000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c65000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200";

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bytes_from_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "hex must be whole bytes");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
        .collect()
}

/// Rust regression lock: `MarmotAction::encode()` produces its OWN canonical
/// payload bytes (present-empty non-optional vector preserved). This pins the
/// Rust encoder; the host parity is checked separately via the semantic
/// round-trip below.
#[test]
fn rust_encode_matches_its_canonical_payload_bytes() {
    let empty = MarmotAction::PublishKeyPackage { relays: vec![] };
    assert_eq!(
        to_hex(&empty.encode()),
        MARMOT_PAYLOAD_PUBLISH_KEY_PACKAGE_EMPTY,
        "Rust PublishKeyPackage{{relays:[]}} encode (present-empty vector) drifted"
    );

    let populated = MarmotAction::CreateGroup {
        name: "Engineering".to_string(),
        description: "Team chat".to_string(),
        invitee_text: Some("npub1abc npub1def".to_string()),
        invitee_npubs: Some(vec!["npub1abc".to_string(), "npub1def".to_string()]),
        signed_key_package_events_json: vec![],
        relays: vec!["wss://relay.example".to_string()],
    };
    assert_eq!(
        to_hex(&populated.encode()),
        MARMOT_PAYLOAD_CREATE_GROUP_POPULATED,
        "Rust CreateGroup populated encode drifted"
    );
}

/// Cross-language SEMANTIC parity on ACTUAL host-builder bytes (#2169 Option B).
///
/// The captured Kotlin/Swift `GeneratedActionBuilders` envelope bytes (which are
/// byte-identical to each other) MUST round-trip through the production decode
/// path (`decode_dispatch_envelope` → `MarmotAction::decode`) to the exact
/// expected `MarmotAction`. This proves the kernel consumes host builder output
/// faithfully — including the present-empty non-optional vectors — even though
/// the host's hand-rolled field order is byte-different from Rust's flatc order.
#[test]
fn host_builder_bytes_round_trip_to_expected_action() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let cases = [
        (
            HOST_ENVELOPE_PUBLISH_KEY_PACKAGE_EMPTY,
            MarmotAction::PublishKeyPackage { relays: vec![] },
        ),
        (
            HOST_ENVELOPE_CREATE_GROUP_POPULATED,
            MarmotAction::CreateGroup {
                name: "Engineering".to_string(),
                description: "Team chat".to_string(),
                invitee_text: Some("npub1abc npub1def".to_string()),
                invitee_npubs: Some(vec!["npub1abc".to_string(), "npub1def".to_string()]),
                signed_key_package_events_json: vec![],
                relays: vec!["wss://relay.example".to_string()],
            },
        ),
    ];

    for (envelope_hex, expected) in cases {
        let bytes = bytes_from_hex(envelope_hex);
        let decoded = decode_dispatch_envelope(&bytes)
            .expect("host envelope bytes must decode via S2 decode_dispatch_envelope");
        assert_eq!(decoded.action_namespace, "nmp.marmot");
        assert_eq!(decoded.correlation_id, "golden-corr");
        let action = MarmotAction::decode(&decoded.payload)
            .expect("host payload must decode via MarmotAction::decode");
        assert_eq!(
            action, expected,
            "host builder bytes must decode to the expected MarmotAction \
             (semantic cross-language parity — #2169)"
        );
    }
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
    let body = fb::Send::create(
        &mut fbb,
        &fb::SendArgs {
            group_id_hex: Some(gid),
            text: Some(text),
        },
    );
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
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn unknown_discriminant_rejected() {
    // Manually craft a root with an unknown union discriminant (255).
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let gid = fbb.create_string("deadbeef1234");
    // Use Leave as a placeholder body (we'll patch the discriminant byte).
    let body = fb::Leave::create(
        &mut fbb,
        &fb::LeaveArgs {
            group_id_hex: Some(gid),
        },
    );
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
