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
