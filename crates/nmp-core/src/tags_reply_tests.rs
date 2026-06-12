//! Tests for [`super::reply_tags`] — the canonical NIP-10 reply-tag builder.
//!
//! Kept separate from the inline `tags.rs` test module to stay within the
//! 500-line file-size ceiling.

use super::*;

#[test]
fn reply_tags_for_root_note_uses_parent_as_both_root_and_reply() {
    // Parent has no root ref → it is the thread root; both e-tags carry
    // parent_id.
    let refs = Nip10Refs::default();
    let tags = reply_tags("PARENT", "alice", &refs, None);
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[0], vec!["e", "PARENT", "", "root"]);
    assert_eq!(tags[1], vec!["e", "PARENT", "", "reply"]);
    assert_eq!(tags[2], vec!["p", "alice"]);
}

#[test]
fn reply_tags_for_mid_thread_note_inherits_root_ref() {
    // Parent carries an existing root — new note inherits it.
    let refs = Nip10Refs {
        root: Some(EventRef {
            id: "ROOT".into(),
            relay: Some("wss://r.root".into()),
            marker: Some("root".into()),
        }),
        reply: None,
        mentions: vec![],
        mentioned_pubkeys: vec!["carol".into()],
    };
    let tags = reply_tags("PARENT", "bob", &refs, None);
    // root e-tag uses root id + root relay
    assert_eq!(tags[0][1], "ROOT");
    assert_eq!(tags[0][2], "wss://r.root");
    assert_eq!(tags[0][3], "root");
    // reply e-tag uses parent_id
    assert_eq!(tags[1][1], "PARENT");
    assert_eq!(tags[1][3], "reply");
    // p-tags: bob first, then carol
    assert_eq!(tags[2][1], "bob");
    assert_eq!(tags[3][1], "carol");
}

#[test]
fn reply_tags_deduplicates_pubkeys() {
    // parent_author == one of mentioned_pubkeys → must not duplicate.
    let refs = Nip10Refs {
        root: None,
        reply: None,
        mentions: vec![],
        mentioned_pubkeys: vec!["alice".into(), "carol".into()],
    };
    let tags = reply_tags("P", "alice", &refs, None);
    let p_ids: Vec<&str> = tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1).map(String::as_str))
        .collect();
    // alice appears once (from parent_author), then carol
    assert_eq!(p_ids, vec!["alice", "carol"]);
}

#[test]
fn reply_tags_relay_hint_appears_on_reply_and_p_tags() {
    let refs = Nip10Refs::default();
    let tags = reply_tags("PARENT", "alice", &refs, Some("wss://hint"));
    // root relay = relay_hint (parent is root, so inherits hint)
    assert_eq!(tags[0][2], "wss://hint");
    // reply relay = relay_hint
    assert_eq!(tags[1][2], "wss://hint");
    // p-tag relay = relay_hint
    assert_eq!(tags[2].len(), 3);
    assert_eq!(tags[2][2], "wss://hint");
}

#[test]
fn reply_tags_root_relay_inherited_not_overridden_by_hint() {
    // When parent already has a root ref with a relay, that relay is
    // kept on the root e-tag regardless of the caller's relay_hint.
    let refs = Nip10Refs {
        root: Some(EventRef {
            id: "ROOT".into(),
            relay: Some("wss://from-parent".into()),
            marker: Some("root".into()),
        }),
        ..Default::default()
    };
    let tags = reply_tags("PARENT", "bob", &refs, Some("wss://hint"));
    assert_eq!(tags[0][2], "wss://from-parent", "root relay from parent");
    assert_eq!(tags[1][2], "wss://hint", "reply relay from hint");
}
