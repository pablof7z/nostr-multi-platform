use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn open_author_emits_profile_and_note_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let requests = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);

    assert_eq!(requests.len(), 3);
    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Indexer));
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Content));
    assert!(joined.contains("\"author-relays-1\""));
    assert!(joined.contains("\"author-profile-1\""));
    assert!(joined.contains("\"author-notes-1\""));
    assert!(joined.contains("\"kinds\":[10002]"));
    assert!(joined.contains("\"kinds\":[0]"));
    assert!(joined.contains("\"kinds\":[1,6]"));
    assert!(joined.contains(FIATJAF_PUBKEY));
    assert!(!kernel.author_request_pending);
}

#[test]
fn open_thread_emits_context_and_reply_reqs() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let focused_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let root_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let previous_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    kernel.events.insert(
        focused_id.to_string(),
        StoredEvent {
            id: focused_id.to_string(),
            author: TEST_PUBKEY.to_string(),
            kind: 1,
            created_at: 1,
            tags: vec![
                vec![
                    "e".to_string(),
                    root_id.to_string(),
                    "".to_string(),
                    "root".to_string(),
                ],
                vec![
                    "e".to_string(),
                    previous_id.to_string(),
                    "".to_string(),
                    "reply".to_string(),
                ],
            ],
            content: "focused".to_string(),
            relay_count: 1,
        },
    );

    let requests = kernel.open_thread(focused_id.to_string(), true);

    assert_eq!(requests.len(), 2);
    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"thread-ids-1\""));
    assert!(joined.contains("\"thread-replies-2\""));
    assert!(joined.contains(focused_id));
    assert!(joined.contains(root_id));
    assert!(joined.contains(previous_id));
    assert!(joined.contains("\"#e\""));
    assert!(!kernel.thread_request_pending);
}

#[test]
fn close_author_refcounts_and_closes_view_subscriptions() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let _ = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);
    let _ = kernel.open_author(FIATJAF_PUBKEY.to_string(), true);

    let first_close = kernel.close_author(FIATJAF_PUBKEY);
    assert!(first_close.is_empty());
    assert_eq!(
        kernel.selected_author.as_ref().map(|view| view.refcount),
        Some(1)
    );

    let second_close = kernel.close_author(FIATJAF_PUBKEY);
    let joined = second_close
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"CLOSE\""));
    assert!(joined.contains("author-profile-1"));
    assert!(joined.contains("author-notes-1"));
    assert!(kernel.selected_author.is_none());
}

#[test]
fn parse_relay_list_splits_nip65_markers() {
    let parsed = parse_relay_list(
        123,
        &[
            vec![
                "r".to_string(),
                "wss://read.example".to_string(),
                "read".to_string(),
            ],
            vec![
                "r".to_string(),
                "wss://write.example".to_string(),
                "write".to_string(),
            ],
            vec!["r".to_string(), "wss://both.example".to_string()],
            vec![
                "r".to_string(),
                "https://not-a-relay.example".to_string(),
                "read".to_string(),
            ],
        ],
    );

    assert_eq!(parsed.created_at, 123);
    assert_eq!(parsed.read_relays, vec!["wss://read.example"]);
    assert_eq!(parsed.write_relays, vec!["wss://write.example"]);
    assert_eq!(parsed.both_relays, vec!["wss://both.example"]);
}
