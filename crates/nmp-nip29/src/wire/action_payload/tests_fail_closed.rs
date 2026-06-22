//! Fail-closed codec tests for the nip29 typed action payloads (ADR-0064 / S9
//! #1747): malformed input and the before-`start()` `schema_version` tripwire.
//! Every assertion is the NEGATIVE — a reject, never a silent decode.

use crate::action::{
    CreateInviteInput, CreatePublicGroupInput, GroupAccess, GroupEventTarget, GroupVisibility,
    JoinGroupInput, LeaveGroupInput, PostChatMessageInput, PutUserInput, ReactInGroupInput,
    RepostInGroupInput, ShareEventInGroupInput,
};
use crate::group_id::GroupId;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room")
}

// --- fail-closed: malformed --------------------------------------------------

#[test]
fn malformed_buffers_are_rejected_for_every_payload() {
    assert!(matches!(
        JoinGroupInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        LeaveGroupInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PostChatMessageInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        ReactInGroupInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CreatePublicGroupInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        ShareEventInGroupInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        RepostInGroupInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PutUserInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CreateInviteInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

// --- fail-closed: wrong schema_version ---------------------------------------
//
// Each payload is decoded from a buffer whose `schema_version` was stamped at a
// bogus value. Because all encoders stamp the compiled `SCHEMA_VERSION`, we
// build the trip buffer by hand-patching the raw schema_version slot (offset 4)
// — the first field in every table — to a value the gate rejects. This proves
// the gate trips BEFORE any field read, for every namespace.

/// Re-encode `bytes` (a finished, file-identified payload) with the raw
/// `schema_version` slot overwritten. The slot is the FIRST table field
/// (`VT_SCHEMA_VERSION = 4`) in every nip29 action table; locating it by reading
/// the root table offset keeps the helper layout-robust without depending on a
/// specific vtable.
fn patch_schema_version(mut bytes: Vec<u8>, new_version: u32) -> Vec<u8> {
    // Root uoffset at [0..4] points to the root table.
    let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    // Table starts with a SIGNED soffset back to its vtable.
    let vtable_soff = i32::from_le_bytes([
        bytes[root_off],
        bytes[root_off + 1],
        bytes[root_off + 2],
        bytes[root_off + 3],
    ]);
    let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
    // vtable: [vtable_size:u16][table_size:u16][field offsets:u16...]
    // VT_SCHEMA_VERSION = 4 -> first field offset entry at vtable_off + 4.
    let field_off = u16::from_le_bytes([bytes[vtable_off + 4], bytes[vtable_off + 5]]) as usize;
    assert_ne!(field_off, 0, "schema_version must be present in the buffer");
    let abs = root_off + field_off;
    bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
    bytes
}

#[test]
fn wrong_schema_version_is_rejected_for_every_payload() {
    macro_rules! assert_bad_version {
        ($ty:ty, $value:expr) => {{
            let good = $value.encode();
            let bad = patch_schema_version(good, 999);
            let err = <$ty>::decode(&bad).expect_err("bad schema_version must be rejected");
            assert_eq!(
                err,
                ActionPayloadDecodeError::SchemaVersionMismatch {
                    found: 999,
                    expected: 1
                },
                "expected a version trip for {}",
                stringify!($ty)
            );
        }};
    }

    assert_bad_version!(
        JoinGroupInput,
        JoinGroupInput {
            group: group(),
            invite_code: None,
            reason: None
        }
    );
    assert_bad_version!(
        LeaveGroupInput,
        LeaveGroupInput {
            group: group(),
            reason: None
        }
    );
    assert_bad_version!(
        PostChatMessageInput,
        PostChatMessageInput {
            group: group(),
            content: "x".to_string(),
            previous_event_id_prefixes: Vec::new(),
            reply_to_event_id: None,
        }
    );
    assert_bad_version!(
        ReactInGroupInput,
        ReactInGroupInput {
            group: group(),
            target_event_id: "t".to_string(),
            target_author_pubkey: None,
            content: "+".to_string(),
        }
    );
    assert_bad_version!(
        CreatePublicGroupInput,
        CreatePublicGroupInput {
            group: group(),
            name: "G".to_string(),
            about: None,
            picture: None,
            visibility: GroupVisibility::Public,
            access: GroupAccess::Open,
        }
    );
    assert_bad_version!(
        ShareEventInGroupInput,
        ShareEventInGroupInput {
            group: group(),
            target: GroupEventTarget {
                event_id: "t".to_string(),
                author_pubkey: None
            },
            content: String::new(),
            additional_tags: Vec::new(),
        }
    );
    assert_bad_version!(
        RepostInGroupInput,
        RepostInGroupInput {
            group: group(),
            target: GroupEventTarget {
                event_id: "t".to_string(),
                author_pubkey: None
            },
            content: String::new(),
            additional_tags: Vec::new(),
        }
    );
    assert_bad_version!(
        PutUserInput,
        PutUserInput {
            group: group(),
            target_pubkey: "a".repeat(64),
            role: None,
            reason: None,
        }
    );
    assert_bad_version!(
        CreateInviteInput,
        CreateInviteInput {
            group: group(),
            codes: vec!["c".to_string()]
        }
    );
}
