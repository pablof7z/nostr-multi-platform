use nmp_core::substrate::{ActionModule, ActionPayload, ActionPayloadDecodeError};

use super::*;
use crate::action::{
    VisibleNoteRelationsAction, VisibleNoteRelationsLifecycle, VisibleNoteRelationsModule,
};

fn action(lifecycle: VisibleNoteRelationsLifecycle) -> VisibleNoteRelationsAction {
    VisibleNoteRelationsAction {
        lifecycle,
        target_event_id: "11".repeat(32),
        target_kind: nmp_kinds::KIND_SHORT_TEXT_NOTE,
        consumer_id: "feed-row".to_string(),
        target_address: None,
    }
}

fn patch_schema_version(mut bytes: Vec<u8>, new_version: u32) -> Vec<u8> {
    let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let vtable_soff = i32::from_le_bytes([
        bytes[root_off],
        bytes[root_off + 1],
        bytes[root_off + 2],
        bytes[root_off + 3],
    ]);
    let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
    let field_off = u16::from_le_bytes([bytes[vtable_off + 4], bytes[vtable_off + 5]]) as usize;
    let abs = root_off + field_off;
    bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
    bytes
}

fn patch_lifecycle_ordinal(mut bytes: Vec<u8>, value: u8) -> Vec<u8> {
    let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let vtable_soff = i32::from_le_bytes([
        bytes[root_off],
        bytes[root_off + 1],
        bytes[root_off + 2],
        bytes[root_off + 3],
    ]);
    let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
    let field_off = u16::from_le_bytes([bytes[vtable_off + 6], bytes[vtable_off + 7]]) as usize;
    assert_ne!(field_off, 0, "lifecycle slot must be present");
    bytes[root_off + field_off] = value;
    bytes
}

#[test]
fn module_and_payload_schema_ids_match() {
    assert_eq!(
        VisibleNoteRelationsModule::NAMESPACE,
        <VisibleNoteRelationsAction as ActionPayload>::SCHEMA_ID
    );
    assert_eq!(VisibleNoteRelationsAction::SCHEMA_VERSION, SCHEMA_VERSION);
}

#[test]
fn claim_round_trips() {
    let action = VisibleNoteRelationsAction {
        target_address: Some(format!("30023:{}:article", "22".repeat(32))),
        ..action(VisibleNoteRelationsLifecycle::Claim)
    };
    assert_eq!(
        VisibleNoteRelationsAction::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn release_round_trips() {
    let action = action(VisibleNoteRelationsLifecycle::Release);
    assert_eq!(
        VisibleNoteRelationsAction::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn malformed_buffer_is_rejected() {
    assert!(matches!(
        VisibleNoteRelationsAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_file_identifier_is_rejected() {
    let mut bytes = action(VisibleNoteRelationsLifecycle::Claim).encode();
    bytes[4..8].copy_from_slice(b"XXXX");
    assert!(matches!(
        VisibleNoteRelationsAction::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_schema_version_is_rejected() {
    let bytes = action(VisibleNoteRelationsLifecycle::Claim).encode();
    let err = VisibleNoteRelationsAction::decode(&patch_schema_version(bytes, 999))
        .expect_err("schema mismatch must reject");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: 1
        }
    );
}

#[test]
fn unknown_lifecycle_ordinal_is_rejected() {
    let bytes = action(VisibleNoteRelationsLifecycle::Release).encode();
    assert!(matches!(
        VisibleNoteRelationsAction::decode(&patch_lifecycle_ordinal(bytes, 99)),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
