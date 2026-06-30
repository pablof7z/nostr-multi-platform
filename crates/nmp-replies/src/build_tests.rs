use super::*;
use crate::ReplyTarget;
use nmp_core::tags::{EventRef, Nip10Refs};
use nmp_nip01::NoteRecord;

const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PARENT_AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PARENT: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn note_parent() -> NoteRecord {
    NoteRecord {
        event_id: PARENT.to_string(),
        author: PARENT_AUTHOR.to_string(),
        created_at: 0,
        content: "parent".to_string(),
        refs: Nip10Refs {
            root: Some(EventRef {
                id: ROOT.to_string(),
                relay: None,
                marker: Some("root".to_string()),
            }),
            reply: None,
            mentions: Vec::new(),
            mentioned_pubkeys: Vec::new(),
        },
    }
}

#[test]
fn note_target_builds_kind1_nip10_reply() {
    let event = Reply::to(ReplyTarget::note(note_parent()), "hello")
        .relay_hint("wss://relay.example")
        .build(AUTHOR, 42)
        .unwrap();

    assert_eq!(event.kind, KIND_SHORT_TEXT_NOTE);
    assert_eq!(event.pubkey, AUTHOR);
    assert_eq!(event.created_at, 42);
    assert_eq!(event.tags[0], vec!["e", ROOT, "", "root"]);
    assert_eq!(
        event.tags[1],
        vec!["e", PARENT, "wss://relay.example", "reply"]
    );
    assert_eq!(
        event.tags[2],
        vec!["p", PARENT_AUTHOR, "wss://relay.example"]
    );
}

#[test]
fn non_note_event_builds_nip22_comment() {
    let target = ReplyTarget::event(PARENT, 30023, Some(PARENT_AUTHOR.to_string())).unwrap();
    let event = Reply::to(target, "comment").build(AUTHOR, 1).unwrap();

    assert_eq!(event.kind, KIND_NIP22_COMMENT);
    assert_eq!(event.tags[0], vec!["E", PARENT]);
    assert_eq!(event.tags[1], vec!["K", "30023"]);
    assert!(event
        .tags
        .contains(&vec!["P".to_string(), PARENT_AUTHOR.to_string()]));
}

#[test]
fn address_and_external_build_nip22_comments() {
    let address = ReplyTarget::address("30023:pubkey:essay", 30023, None).unwrap();
    let address_event = Reply::to(address, "nice").build(AUTHOR, 1).unwrap();
    assert_eq!(address_event.tags[0], vec!["A", "30023:pubkey:essay"]);
    assert_eq!(address_event.tags[2], vec!["a", "30023:pubkey:essay"]);

    let external = ReplyTarget::external("podcast:item:guid:abc").unwrap();
    let external_event = Reply::to(external, "nice").build(AUTHOR, 1).unwrap();
    assert_eq!(external_event.tags[0], vec!["I", "podcast:item:guid:abc"]);
    assert_eq!(external_event.tags[1], vec!["K", "0"]);
}
