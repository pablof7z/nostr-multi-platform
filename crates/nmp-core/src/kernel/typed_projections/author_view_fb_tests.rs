//! Round-trip proof for the `author_view` Tier-2 typed codec.

use super::*;

fn timeline_item(populated: bool) -> TimelineItemModel {
    if populated {
        TimelineItemModel {
            id: "e".repeat(64),
            author_pubkey: "a".repeat(64),
            author_picture_url: Some("https://img/a.png".to_string()),
            author_lnurl: Some("a@wos.com".to_string()),
            author_display_name: Some("Alice".to_string()),
            kind: 1,
            content: "gm".to_string(),
            content_preview: "gm".to_string(),
            created_at: 1_700_000_000,
            relay_count: 3,
            is_repost: false,
            nav_target_id: "e".repeat(64),
            repost_inner_content: String::new(),
        }
    } else {
        // kind:6 repost row with every author Option absent.
        TimelineItemModel {
            id: "f".repeat(64),
            author_pubkey: "b".repeat(64),
            author_picture_url: None,
            author_lnurl: None,
            author_display_name: None,
            kind: 6,
            content: "{}".to_string(),
            content_preview: String::new(),
            created_at: 1_700_000_500,
            relay_count: 0,
            is_repost: true,
            nav_target_id: "g".repeat(64),
            repost_inner_content: "inner".to_string(),
        }
    }
}

fn profile() -> ProfileCardModel {
    ProfileCardModel {
        pubkey: "a".repeat(64),
        npub: "npub1aaa".to_string(),
        display_name: Some("Alice".to_string()),
        picture_url: None,
        nip05: String::new(),
        about: "about".to_string(),
        has_profile: true,
        lnurl: None,
    }
}

/// Write action (follow) with a nested dispatch spec.
fn sample_with_write_action() -> AuthorViewModel {
    AuthorViewModel {
        pubkey: "a".repeat(64),
        state: "ready".to_string(),
        profile: profile(),
        items: vec![timeline_item(true), timeline_item(false)],
        note_count: 2,
        note_count_display: "2".to_string(),
        primary_action: Some(ProfileActionModel {
            kind: "follow".to_string(),
            label: "Follow".to_string(),
            target_pubkey: "a".repeat(64),
            icon_name: "person.badge.plus".to_string(),
            dispatch: Some(ProfileDispatchSpecModel {
                namespace: "nmp.follow".to_string(),
                body_json: r#"{"pubkey":"aaa"}"#.to_string(),
            }),
        }),
    }
}

/// Local-UI intent (edit) — action present but `dispatch: None`.
fn sample_with_local_action() -> AuthorViewModel {
    let mut model = sample_with_write_action();
    model.primary_action = Some(ProfileActionModel {
        kind: "edit_profile".to_string(),
        label: "Edit".to_string(),
        target_pubkey: "a".repeat(64),
        icon_name: "square.and.pencil".to_string(),
        dispatch: None,
    });
    model
}

/// No primary action at all.
fn sample_without_action() -> AuthorViewModel {
    let mut model = sample_with_write_action();
    model.primary_action = None;
    model.items.clear();
    model.note_count = 0;
    model.note_count_display = "0".to_string();
    model
}

#[test]
fn write_action_round_trips_with_nested_dispatch() {
    let model = sample_with_write_action();
    let bytes = encode_author_view(&model);
    let decoded = decode_author_view(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    let action = decoded.primary_action.expect("primary_action present");
    assert!(action.dispatch.is_some(), "write action carries a dispatch");
}

#[test]
fn local_intent_action_round_trips_without_dispatch() {
    let model = sample_with_local_action();
    let bytes = encode_author_view(&model);
    let decoded = decode_author_view(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    let action = decoded.primary_action.expect("primary_action present");
    assert!(
        action.dispatch.is_none(),
        "local-intent action has no dispatch (mirrors JSON `dispatch: null`)"
    );
}

#[test]
fn absent_primary_action_and_empty_items_round_trip() {
    let model = sample_without_action();
    let bytes = encode_author_view(&model);
    let decoded = decode_author_view(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.primary_action.is_none());
    assert!(decoded.items.is_empty());
}

#[test]
fn timeline_item_options_survive_distinctly() {
    let model = sample_with_write_action();
    let bytes = encode_author_view(&model);
    let decoded = decode_author_view(&bytes).expect("decode must succeed");
    assert!(decoded.items[0].author_display_name.is_some());
    assert!(decoded.items[1].author_display_name.is_none());
    assert!(decoded.items[1].is_repost);
    assert_eq!(decoded.items[1].repost_inner_content, "inner");
}

#[test]
fn buffer_carries_the_kavw_file_identifier() {
    let bytes = encode_author_view(&sample_with_write_action());
    assert_eq!(
        &bytes[4..8],
        AUTHOR_VIEW_FILE_IDENTIFIER,
        "the buffer must embed the KAVW file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_author_view(&[]).is_err());
    assert!(decode_author_view(b"NMPU0000").is_err());
}
