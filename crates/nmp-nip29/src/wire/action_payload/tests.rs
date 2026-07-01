//! Round-trip + fail-closed codec tests for the nip29 typed action payloads
//! (ADR-0064 / S9 #1747). Every fail-closed gate asserts the NEGATIVE; every
//! optional-string PRESENCE invariant asserts both `None` and `Some("")`.

use crate::action::{
    CreateGroupInput, CreateInviteInput, DiscoverGroupsInput, EditMetadataInput, GroupAccess,
    GroupVisibility, JoinGroupInput, LeaveGroupInput, PublishGroupEventInput, PutUserInput,
    SetParentInput,
};
use crate::group_id::GroupId;
use nmp_core::substrate::ActionPayload;

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room")
}

// --- join --------------------------------------------------------------------

#[test]
fn join_round_trips_with_code_and_reason() {
    let action = JoinGroupInput {
        group: group(),
        invite_code: Some("secret-1".to_string()),
        reason: Some("please".to_string()),
    };
    assert_eq!(
        JoinGroupInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn join_preserves_optional_presence() {
    // Absent -> None.
    let absent = JoinGroupInput {
        group: group(),
        invite_code: None,
        reason: None,
    };
    let d = JoinGroupInput::decode(&absent.encode()).expect("decodes");
    assert!(d.invite_code.is_none() && d.reason.is_none());
    // Present-empty -> Some("") (NOT collapsed to None).
    let empty = JoinGroupInput {
        group: group(),
        invite_code: Some(String::new()),
        reason: Some(String::new()),
    };
    let d = JoinGroupInput::decode(&empty.encode()).expect("decodes");
    assert_eq!(d.invite_code.as_deref(), Some(""));
    assert_eq!(d.reason.as_deref(), Some(""));
}

// --- leave -------------------------------------------------------------------

#[test]
fn leave_round_trips_and_preserves_presence() {
    let with_reason = LeaveGroupInput {
        group: group(),
        reason: Some("bye".to_string()),
    };
    assert_eq!(
        LeaveGroupInput::decode(&with_reason.encode()).expect("decodes"),
        with_reason
    );
    let none = LeaveGroupInput {
        group: group(),
        reason: None,
    };
    assert!(LeaveGroupInput::decode(&none.encode())
        .expect("decodes")
        .reason
        .is_none());
    let empty = LeaveGroupInput {
        group: group(),
        reason: Some(String::new()),
    };
    assert_eq!(
        LeaveGroupInput::decode(&empty.encode())
            .expect("decodes")
            .reason
            .as_deref(),
        Some("")
    );
}

// --- publish_group_event -----------------------------------------------------

#[test]
fn publish_round_trips_with_kind_content_and_tags() {
    let action = PublishGroupEventInput {
        group: group(),
        kind: 9,
        content: "hello".to_string(),
        tags: vec![
            vec![
                "e".to_string(),
                "cc".to_string(),
                String::new(),
                "reply".to_string(),
            ],
            vec!["t".to_string(), "nostr".to_string()],
        ],
    };
    assert_eq!(
        PublishGroupEventInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn publish_preserves_empty_content_and_empty_tags() {
    let action = PublishGroupEventInput {
        group: group(),
        kind: 11,
        content: String::new(),
        tags: Vec::new(),
    };
    let d = PublishGroupEventInput::decode(&action.encode()).expect("decodes");
    assert_eq!(d.kind, 11);
    assert!(d.content.is_empty());
    assert!(d.tags.is_empty());
}

// --- create_group -----------------------------------------------------

#[test]
fn create_round_trips_all_fields() {
    let action = CreateGroupInput {
        group: group(),
        name: "Rust Nostr".to_string(),
        about: Some("a group".to_string()),
        picture: Some("https://x/p.png".to_string()),
        visibility: GroupVisibility::Private,
        access: GroupAccess::Closed,
        parent: Some("tech".to_string()),
    };
    assert_eq!(
        CreateGroupInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn create_defaults_and_presence() {
    let action = CreateGroupInput {
        group: group(),
        name: "G".to_string(),
        about: None,
        picture: Some(String::new()),
        visibility: GroupVisibility::Public,
        access: GroupAccess::Open,
        parent: None,
    };
    let d = CreateGroupInput::decode(&action.encode()).expect("decodes");
    assert!(d.about.is_none());
    assert_eq!(d.picture.as_deref(), Some(""));
    assert_eq!(d.visibility, GroupVisibility::Public);
    assert_eq!(d.access, GroupAccess::Open);
    assert!(d.parent.is_none());
}

// --- put_user ----------------------------------------------------------------

#[test]
fn put_user_round_trips_and_preserves_role_presence() {
    let with_role = PutUserInput {
        group: group(),
        target_pubkey: "a".repeat(64),
        role: Some("admin".to_string()),
        reason: Some("promote".to_string()),
    };
    assert_eq!(
        PutUserInput::decode(&with_role.encode()).expect("decodes"),
        with_role
    );
    // None role.
    let none = PutUserInput {
        role: None,
        reason: None,
        ..with_role.clone()
    };
    let d = PutUserInput::decode(&none.encode()).expect("decodes");
    assert!(d.role.is_none() && d.reason.is_none());
    // PRESENCE-CRITICAL: present-empty role must NOT collapse to None — a
    // collapse would bypass the `start()` "role must not be empty" check.
    let empty_role = PutUserInput {
        role: Some(String::new()),
        ..none
    };
    assert_eq!(
        PutUserInput::decode(&empty_role.encode())
            .expect("decodes")
            .role
            .as_deref(),
        Some("")
    );
}

// --- create_invite -----------------------------------------------------------

#[test]
fn create_invite_round_trips() {
    let action = CreateInviteInput {
        group: group(),
        codes: vec!["c1".to_string(), "c2".to_string(), "c3".to_string()],
    };
    assert_eq!(
        CreateInviteInput::decode(&action.encode()).expect("decodes"),
        action
    );
    // Empty codes vector round-trips as empty (shape decode; `start()` rejects).
    let empty = CreateInviteInput {
        group: group(),
        codes: Vec::new(),
    };
    assert!(CreateInviteInput::decode(&empty.encode())
        .expect("decodes")
        .codes
        .is_empty());
}

// --- discover_groups ---------------------------------------------------------

#[test]
fn discover_groups_round_trips() {
    let action = DiscoverGroupsInput {
        relay_url: "wss://groups.example.com".to_string(),
    };
    assert_eq!(
        DiscoverGroupsInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn discover_groups_wss_and_ws_schemes_survive_round_trip() {
    for url in &["wss://groups.example.com", "ws://localhost:7777"] {
        let action = DiscoverGroupsInput {
            relay_url: url.to_string(),
        };
        let decoded = DiscoverGroupsInput::decode(&action.encode()).expect("decodes");
        assert_eq!(&decoded.relay_url, url);
    }
}

// --- set_parent (NIP-29 subgroups, nips PR #2319) ────────────────────────────

#[test]
fn set_parent_round_trips_adopt() {
    let action = SetParentInput {
        group: group(),
        parent: Some("tech".to_string()),
    };
    let decoded = SetParentInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert_eq!(decoded.parent.as_deref(), Some("tech"));
}

#[test]
fn set_parent_round_trips_detach() {
    // `parent: None` detaches to root — absent on the wire.
    let action = SetParentInput {
        group: group(),
        parent: None,
    };
    let decoded = SetParentInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.parent.is_none());
}

#[test]
fn set_parent_present_empty_string_is_preserved() {
    // present-empty vs absent distinction: `Some("")` is a present (if
    // empty) parent; the wire codec preserves it so `start()`'s trim guard
    // sees the caller's intent.
    let action = SetParentInput {
        group: group(),
        parent: Some(String::new()),
    };
    let decoded = SetParentInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded.parent.as_deref(), Some(""));
}

// --- edit_metadata ───────────────────────────────────────────────────────────

#[test]
fn edit_metadata_round_trips_all_fields() {
    let action = EditMetadataInput {
        group: group(),
        name: Some("Renamed".to_string()),
        about: Some("New about".to_string()),
        picture: Some("https://x/p.png".to_string()),
        visibility: Some(GroupVisibility::Private),
        access: Some(GroupAccess::Closed),
    };
    let decoded = EditMetadataInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn edit_metadata_tri_state_unset_decodes_to_none() {
    // None visibility/access must round-trip as None (Unset on the wire), so a
    // partial edit never silently flips the group's visibility/access.
    let action = EditMetadataInput {
        group: group(),
        name: Some("Just a rename".to_string()),
        about: None,
        picture: None,
        visibility: None,
        access: None,
    };
    let decoded = EditMetadataInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.visibility.is_none() && decoded.access.is_none());
    assert!(decoded.about.is_none() && decoded.picture.is_none());
}

#[test]
fn edit_metadata_present_empty_string_is_preserved() {
    let action = EditMetadataInput {
        group: group(),
        name: Some(String::new()),
        about: None,
        picture: None,
        visibility: None,
        access: None,
    };
    let decoded = EditMetadataInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded.name.as_deref(), Some(""));
}
