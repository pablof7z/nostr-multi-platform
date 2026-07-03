use super::*;

#[test]
fn round_trips_full_snapshot() {
    let snapshot = ChatPresenceSnapshot {
        host_relay_url: "wss://groups.example.com".into(),
        group_id: "room-a".into(),
        active_pubkey: "me".into(),
        read_marker: Some(ReadMarker::new("event-1", 42)),
        unread_count: 3,
        typing: vec![ChatPresenceTyping {
            pubkey: "alice".into(),
            updated_at_ms: 100,
            expires_at_ms: 200,
        }],
    };

    let bytes = encode_chat_presence_snapshot(&snapshot);
    assert!(generated::nmp::chat::chat_presence_snapshot_buffer_has_identifier(&bytes));
    assert_eq!(decode_chat_presence_snapshot(&bytes).unwrap(), snapshot);
}

#[test]
fn round_trips_absent_read_marker() {
    let snapshot = ChatPresenceSnapshot {
        host_relay_url: "wss://groups.example.com".into(),
        group_id: "room-a".into(),
        active_pubkey: "me".into(),
        read_marker: None,
        unread_count: 0,
        typing: Vec::new(),
    };

    assert_eq!(
        decode_chat_presence_snapshot(&encode_chat_presence_snapshot(&snapshot)).unwrap(),
        snapshot
    );
}
