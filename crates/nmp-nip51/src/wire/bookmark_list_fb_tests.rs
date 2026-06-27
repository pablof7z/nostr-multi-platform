use crate::{decode_bookmark_list, encode_bookmark_list, BookmarkItem, BookmarkListSnapshot};

#[test]
fn bookmark_list_roundtrips_items() {
    let snapshot = BookmarkListSnapshot {
        items: vec![
            BookmarkItem::Event {
                event_id: "e".repeat(64),
                relay: Some("wss://relay.example".to_string()),
            },
            BookmarkItem::Address {
                coordinate: format!("30023:{}:slug", "a".repeat(64)),
                relay: None,
            },
            BookmarkItem::Url {
                url: "https://example.com/read".to_string(),
            },
            BookmarkItem::Hashtag {
                hashtag: "nostr".to_string(),
            },
        ],
        metadata: Default::default(),
    };

    let bytes = encode_bookmark_list(&snapshot);
    let decoded = decode_bookmark_list(&bytes).expect("bookmark list decodes");
    assert_eq!(decoded, snapshot);
}
