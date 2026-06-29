//! Registry-level trip tests for the nip29 typed FlatBuffers payload doorway
//! (ADR-0064 / S9 #1747).
//!
//! Two contracts, per migrated namespace:
//!
//! 1. NEGATIVE — a payload whose `schema_version` is bad MUST be rejected by
//!    `ActionRegistry::start_bytes` BEFORE `start()` runs (fail-closed).
//! 2. POSITIVE — a well-formed typed payload round-trips through `start_bytes`
//!    (the module's `decode_payload` override is wired AND its `start()`
//!    validator accepts the decoded action), proving the typed doorway is live
//!    end-to-end.
//!
//! Codec round-trip + per-field presence tests live in
//! `src/wire/action_payload/tests.rs`. These tests sit one level up — at the
//! registry boundary — so they exercise the same path the byte transport
//! (S2 `DispatchEnvelope`) drives in production.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};

use nmp_nip29::GroupId;
use nmp_nip29::action::{
    CreateInviteAction, CreateInviteInput, CreatePublicGroupAction, CreatePublicGroupInput,
    EditMetadataAction, EditMetadataInput, GroupAccess, GroupEventTarget, GroupVisibility,
    JoinGroupAction, JoinGroupInput, LeaveGroupAction, LeaveGroupInput, PublishGroupEventAction,
    PublishGroupEventInput, PutUserAction, PutUserInput, ReactInGroupAction, ReactInGroupInput,
    RepostInGroupAction, RepostInGroupInput, ShareEventInGroupAction, ShareEventInGroupInput,
};

/// Register every migrated nip29 event-authoring module onto a fresh registry.
fn registry() -> ActionRegistry {
    let mut r = ActionRegistry::new();
    r.register(JoinGroupAction).expect("join registers");
    r.register(LeaveGroupAction).expect("leave registers");
    r.register(PublishGroupEventAction)
        .expect("publish group event registers");
    r.register(ReactInGroupAction)
        .expect("react in group registers");
    r.register(CreatePublicGroupAction)
        .expect("create public group registers");
    r.register(ShareEventInGroupAction)
        .expect("share event in group registers");
    r.register(RepostInGroupAction)
        .expect("repost in group registers");
    r.register(PutUserAction).expect("put user registers");
    r.register(CreateInviteAction)
        .expect("create invite registers");
    r.register(EditMetadataAction)
        .expect("edit metadata registers");
    r
}

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room")
}

/// Overwrite the raw `schema_version` slot (the FIRST table field,
/// `VT_SCHEMA_VERSION = 4`) of a finished payload buffer with a bad value,
/// without touching any other bytes. Proves the gate trips on the raw value.
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
    assert_ne!(field_off, 0, "schema_version must be present");
    let abs = root_off + field_off;
    bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
    bytes
}

fn assert_bad_version_rejected(namespace: &str, good_bytes: Vec<u8>) {
    let registry = registry();
    let bad = patch_schema_version(good_bytes, 999);
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            namespace,
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "{namespace}: rejection must name the version trip: {msg}"
        ),
        other => panic!("{namespace}: expected Invalid rejection, got {other:?}"),
    }
}

fn assert_good_accepted(namespace: &str, good_bytes: Vec<u8>) {
    let registry = registry();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            namespace,
            &good_bytes,
        )
        .unwrap_or_else(|e| {
            panic!("{namespace}: well-formed typed payload must be accepted: {e:?}")
        });
}

// ---- per-namespace fixtures (well-formed, accepted by start()) --------------

fn join() -> JoinGroupInput {
    JoinGroupInput {
        group: group(),
        invite_code: Some("code".into()),
        reason: None,
    }
}
fn leave() -> LeaveGroupInput {
    LeaveGroupInput {
        group: group(),
        reason: Some("bye".into()),
    }
}
fn publish() -> PublishGroupEventInput {
    PublishGroupEventInput {
        group: group(),
        kind: 9,
        content: "hello".into(),
        tags: vec![vec!["t".into(), "nostr".into()]],
    }
}
fn react() -> ReactInGroupInput {
    ReactInGroupInput {
        group: group(),
        target_event_id: "deadbeef".into(),
        target_author_pubkey: Some("auth".into()),
        content: "+".into(),
    }
}
fn create() -> CreatePublicGroupInput {
    CreatePublicGroupInput {
        group: GroupId::new("wss://groups.example.com", "rust-nostr"),
        name: "Rust Nostr".into(),
        about: Some("about".into()),
        picture: None,
        visibility: GroupVisibility::Public,
        access: GroupAccess::Open,
        parent: None,
    }
}
fn share() -> ShareEventInGroupInput {
    ShareEventInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "tid".into(),
            author_pubkey: Some("a".into()),
        },
        content: "shared".into(),
        additional_tags: vec![vec!["t".into(), "nostr".into()]],
    }
}
fn repost() -> RepostInGroupInput {
    RepostInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "tid".into(),
            author_pubkey: None,
        },
        content: String::new(),
        additional_tags: Vec::new(),
    }
}
fn put_user() -> PutUserInput {
    PutUserInput {
        group: group(),
        target_pubkey: "a".repeat(64),
        role: Some("admin".into()),
        reason: None,
    }
}
fn create_invite() -> CreateInviteInput {
    CreateInviteInput {
        group: group(),
        codes: vec!["code-1".into()],
    }
}
fn edit_metadata() -> EditMetadataInput {
    EditMetadataInput {
        group: group(),
        name: Some("Renamed".into()),
        about: Some("New about".into()),
        picture: None,
        visibility: Some(GroupVisibility::Private),
        access: Some(GroupAccess::Closed),
    }
}

// ---- NEGATIVE: bad-version trip for every migrated namespace ----------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_every_namespace() {
    assert_bad_version_rejected("nmp.nip29.join", join().encode());
    assert_bad_version_rejected("nmp.nip29.leave", leave().encode());
    assert_bad_version_rejected("nmp.nip29.publish_group_event", publish().encode());
    assert_bad_version_rejected("nmp.nip29.react_in_group", react().encode());
    assert_bad_version_rejected("nmp.nip29.create_public_group", create().encode());
    assert_bad_version_rejected("nmp.nip29.share_event_in_group", share().encode());
    assert_bad_version_rejected("nmp.nip29.repost_in_group", repost().encode());
    assert_bad_version_rejected("nmp.nip29.put_user", put_user().encode());
    assert_bad_version_rejected("nmp.nip29.create_invite", create_invite().encode());
    assert_bad_version_rejected("nmp.nip29.edit_metadata", edit_metadata().encode());
}

// ---- POSITIVE: well-formed typed payload round-trips through start_bytes ----

#[test]
fn start_bytes_accepts_well_formed_typed_payload_for_every_namespace() {
    assert_good_accepted("nmp.nip29.join", join().encode());
    assert_good_accepted("nmp.nip29.leave", leave().encode());
    assert_good_accepted("nmp.nip29.publish_group_event", publish().encode());
    assert_good_accepted("nmp.nip29.react_in_group", react().encode());
    assert_good_accepted("nmp.nip29.create_public_group", create().encode());
    assert_good_accepted("nmp.nip29.share_event_in_group", share().encode());
    assert_good_accepted("nmp.nip29.repost_in_group", repost().encode());
    assert_good_accepted("nmp.nip29.put_user", put_user().encode());
    assert_good_accepted("nmp.nip29.create_invite", create_invite().encode());
    assert_good_accepted("nmp.nip29.edit_metadata", edit_metadata().encode());
}

// ---- the typed doorway carries malformed bytes through as a fail-closed
//      Invalid rejection (the module never sees the action) -------------------

#[test]
fn start_bytes_rejects_malformed_payload() {
    let registry = registry();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip29.join",
            b"junk",
        )
        .expect_err("malformed bytes must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

/// Presence-preservation guard (the nip57 fail-open lesson): `put_user`'s
/// `start()` rejects a present-but-empty `role` (`Some("")`). If the codec
/// collapsed `Some("")` -> `None` on decode, the typed buffer would decode to an
/// `absent` role and SILENTLY PASS the validator — a fail-open. This proves the
/// typed doorway carries `Some("")` through to `start()`, which then rejects it.
#[test]
fn start_bytes_put_user_rejects_present_empty_role_no_fail_open() {
    let registry = registry();
    let bytes = PutUserInput {
        group: group(),
        target_pubkey: "a".repeat(64),
        role: Some(String::new()),
        reason: None,
    }
    .encode();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip29.put_user",
            &bytes,
        )
        .expect_err("a present-but-empty role must be rejected (no fail-open collapse to None)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("role must not be empty"),
            "rejection must name the empty-role check: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}
