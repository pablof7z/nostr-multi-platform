use super::*;

const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT_EVENT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PARENT_COMMENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const ROOT_AUTHOR: &str = "5555555555555555555555555555555555555555555555555555555555555555";
const PARENT_AUTHOR: &str = "6666666666666666666666666666666666666666666666666666666666666666";

#[test]
fn top_level_event_comment_mirrors_root_scope() {
    let event = build_comment_event(
        CommentBuildInput::top_level(
            CommentRoot::Event {
                event_id: ROOT_EVENT.to_string(),
                kind: 11,
                author_pubkey: Some(ROOT_AUTHOR.to_string()),
            },
            "  hello thread  ",
        ),
        AUTHOR,
        42,
    )
    .unwrap();

    assert_eq!(event.pubkey, AUTHOR);
    assert_eq!(event.kind, KIND_NIP22_COMMENT);
    assert_eq!(event.created_at, 42);
    assert_eq!(event.content, "hello thread");
    assert_eq!(event.tags[0], vec!["E".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(event.tags[1], vec!["K".to_string(), "11".to_string()]);
    assert!(event
        .tags
        .contains(&vec!["P".to_string(), ROOT_AUTHOR.to_string()]));
    assert!(event
        .tags
        .contains(&vec!["e".to_string(), ROOT_EVENT.to_string()]));
    assert!(event
        .tags
        .contains(&vec!["k".to_string(), "11".to_string()]));
}

#[test]
fn child_comment_points_parent_at_comment_event() {
    let event = build_comment_event(
        CommentBuildInput {
            root: CommentRoot::Event {
                event_id: ROOT_EVENT.to_string(),
                kind: 11,
                author_pubkey: None,
            },
            parent: CommentParent::Comment {
                event_id: PARENT_COMMENT.to_string(),
                author_pubkey: Some(PARENT_AUTHOR.to_string()),
            },
            content: "good point".to_string(),
        },
        AUTHOR,
        1,
    )
    .unwrap();

    assert_eq!(event.tags[0], vec!["E".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(
        event.tags[2],
        vec!["e".to_string(), PARENT_COMMENT.to_string()]
    );
    assert_eq!(event.tags[3], vec!["k".to_string(), "1111".to_string()]);
    assert!(event
        .tags
        .contains(&vec!["p".to_string(), PARENT_AUTHOR.to_string()]));
}

#[test]
fn address_and_external_roots_choose_their_scope() {
    let address = build_comment_event(
        CommentBuildInput::top_level(
            CommentRoot::Address {
                coordinate: "30023:pubkey:essay".to_string(),
                kind: 30023,
                author_pubkey: None,
            },
            "nice",
        ),
        AUTHOR,
        1,
    )
    .unwrap();
    assert_eq!(
        address.tags[0],
        vec!["A".to_string(), "30023:pubkey:essay".to_string()]
    );
    assert_eq!(
        address.tags[2],
        vec!["a".to_string(), "30023:pubkey:essay".to_string()]
    );

    let external = build_comment_event(
        CommentBuildInput::top_level(
            CommentRoot::External {
                uri: "podcast:item:guid:abc".to_string(),
            },
            "loved it",
        ),
        AUTHOR,
        1,
    )
    .unwrap();
    assert_eq!(
        external.tags[0],
        vec!["I".to_string(), "podcast:item:guid:abc".to_string()]
    );
    assert_eq!(external.tags[1], vec!["K".to_string(), "0".to_string()]);
    assert_eq!(
        external.tags[2],
        vec!["i".to_string(), "podcast:item:guid:abc".to_string()]
    );
}

#[test]
fn reply_to_comment_reuses_decoded_root_without_tag_inputs() {
    let parent = CommentRecord {
        event_id: PARENT_COMMENT.to_string(),
        author_pubkey: PARENT_AUTHOR.to_string(),
        body: "parent".to_string(),
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT_EVENT.to_string(),
        root_kind: "11".to_string(),
        root_author_pubkey: ROOT_AUTHOR.to_string(),
        parent_tag_name: "e".to_string(),
        parent_tag_value: ROOT_EVENT.to_string(),
        parent_kind: "11".to_string(),
        created_at: 1,
    };

    let input = CommentBuildInput::reply_to_comment(&parent, "child").unwrap();
    let event = build_comment_event(input, AUTHOR, 2).unwrap();

    assert_eq!(event.tags[0], vec!["E".to_string(), ROOT_EVENT.to_string()]);
    assert_eq!(
        event.tags[3],
        vec!["e".to_string(), PARENT_COMMENT.to_string()]
    );
    assert!(event
        .tags
        .contains(&vec!["P".to_string(), ROOT_AUTHOR.to_string()]));
}

#[test]
fn rejects_blank_content_and_malformed_event_ids() {
    assert_eq!(
        build_comment_event(
            CommentBuildInput::top_level(
                CommentRoot::Event {
                    event_id: ROOT_EVENT.to_string(),
                    kind: 1,
                    author_pubkey: None,
                },
                "   ",
            ),
            AUTHOR,
            1,
        )
        .unwrap_err(),
        CommentBuildError::EmptyContent
    );

    assert!(matches!(
        build_comment_event(
            CommentBuildInput::top_level(
                CommentRoot::Event {
                    event_id: "not-hex".to_string(),
                    kind: 1,
                    author_pubkey: None,
                },
                "hi",
            ),
            AUTHOR,
            1,
        ),
        Err(CommentBuildError::InvalidEventId { .. })
    ));
}
