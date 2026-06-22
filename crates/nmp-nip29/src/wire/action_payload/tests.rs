//! Round-trip + fail-closed codec tests for the nip29 typed action payloads
//! (ADR-0064 / S9 #1747). Every fail-closed gate asserts the NEGATIVE; every
//! optional-string PRESENCE invariant asserts both `None` and `Some("")`.

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

// --- post_chat_message -------------------------------------------------------

#[test]
fn chat_round_trips_with_prefixes_and_reply() {
    let action = PostChatMessageInput {
        group: group(),
        content: "hello".to_string(),
        previous_event_id_prefixes: vec!["aa".to_string(), "bb".to_string()],
        reply_to_event_id: Some("cc".to_string()),
    };
    assert_eq!(
        PostChatMessageInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn chat_preserves_reply_presence_and_empty_prefixes() {
    let action = PostChatMessageInput {
        group: group(),
        content: "x".to_string(),
        previous_event_id_prefixes: Vec::new(),
        reply_to_event_id: None,
    };
    let d = PostChatMessageInput::decode(&action.encode()).expect("decodes");
    assert!(d.previous_event_id_prefixes.is_empty());
    assert!(d.reply_to_event_id.is_none());
    let empty_reply = PostChatMessageInput {
        reply_to_event_id: Some(String::new()),
        ..action
    };
    assert_eq!(
        PostChatMessageInput::decode(&empty_reply.encode())
            .expect("decodes")
            .reply_to_event_id
            .as_deref(),
        Some("")
    );
}

// --- create_public_group -----------------------------------------------------

#[test]
fn create_round_trips_all_fields() {
    let action = CreatePublicGroupInput {
        group: group(),
        name: "Rust Nostr".to_string(),
        about: Some("a group".to_string()),
        picture: Some("https://x/p.png".to_string()),
        visibility: GroupVisibility::Private,
        access: GroupAccess::Closed,
    };
    assert_eq!(
        CreatePublicGroupInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn create_defaults_and_presence() {
    let action = CreatePublicGroupInput {
        group: group(),
        name: "G".to_string(),
        about: None,
        picture: Some(String::new()),
        visibility: GroupVisibility::Public,
        access: GroupAccess::Open,
    };
    let d = CreatePublicGroupInput::decode(&action.encode()).expect("decodes");
    assert!(d.about.is_none());
    assert_eq!(d.picture.as_deref(), Some(""));
    assert_eq!(d.visibility, GroupVisibility::Public);
    assert_eq!(d.access, GroupAccess::Open);
}

// --- react_in_group ----------------------------------------------------------

#[test]
fn react_round_trips_and_preserves_author_presence() {
    let with_author = ReactInGroupInput {
        group: group(),
        target_event_id: "deadbeef".to_string(),
        target_author_pubkey: Some("author".to_string()),
        content: "+".to_string(),
    };
    assert_eq!(
        ReactInGroupInput::decode(&with_author.encode()).expect("decodes"),
        with_author
    );
    let none = ReactInGroupInput {
        target_author_pubkey: None,
        ..with_author.clone()
    };
    assert!(ReactInGroupInput::decode(&none.encode())
        .expect("decodes")
        .target_author_pubkey
        .is_none());
    let empty = ReactInGroupInput {
        target_author_pubkey: Some(String::new()),
        ..with_author
    };
    assert_eq!(
        ReactInGroupInput::decode(&empty.encode())
            .expect("decodes")
            .target_author_pubkey
            .as_deref(),
        Some("")
    );
}

// --- share_event_in_group / repost_in_group ----------------------------------

#[test]
fn share_round_trips_with_tags_and_author() {
    let action = ShareEventInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "tid".to_string(),
            author_pubkey: Some("auth".to_string()),
        },
        content: "shared".to_string(),
        additional_tags: vec![
            vec!["t".to_string(), "nostr".to_string()],
            vec!["alt".to_string()],
        ],
    };
    assert_eq!(
        ShareEventInGroupInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn share_preserves_author_presence_and_empty_tags() {
    let action = ShareEventInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "tid".to_string(),
            author_pubkey: None,
        },
        content: String::new(),
        additional_tags: Vec::new(),
    };
    let d = ShareEventInGroupInput::decode(&action.encode()).expect("decodes");
    assert!(d.target.author_pubkey.is_none());
    assert!(d.additional_tags.is_empty());
    assert_eq!(d.content, "");
}

#[test]
fn repost_round_trips() {
    let action = RepostInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "tid".to_string(),
            author_pubkey: Some("auth".to_string()),
        },
        content: "rp".to_string(),
        additional_tags: vec![vec!["e".to_string(), "x".to_string()]],
    };
    assert_eq!(
        RepostInGroupInput::decode(&action.encode()).expect("decodes"),
        action
    );
}

#[test]
fn share_and_repost_identifiers_are_distinct() {
    // A share buffer must NOT decode as a repost (distinct file identifiers).
    let share = ShareEventInGroupInput {
        group: group(),
        target: GroupEventTarget {
            event_id: "t".to_string(),
            author_pubkey: None,
        },
        content: String::new(),
        additional_tags: Vec::new(),
    };
    let bytes = share.encode();
    assert!(matches!(
        RepostInGroupInput::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
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
