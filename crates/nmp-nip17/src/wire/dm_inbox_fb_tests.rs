//! Round-trip + envelope tests for the `dm_inbox` typed FlatBuffers codec.

use super::*;
use crate::inbox::{DmConversation, DmInboxSnapshot, DmMessage};

fn sample_message(id: &str, reply_to: Option<&str>, outgoing: bool) -> DmMessage {
    DmMessage {
        id: id.to_string(),
        sender_pubkey: "a".repeat(64),
        content: "hello world".to_string(),
        created_at: 1_700_000_000,
        reply_to: reply_to.map(str::to_string),
        is_outgoing: outgoing,
        source_relays: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
    }
}

fn sample_snapshot() -> DmInboxSnapshot {
    DmInboxSnapshot {
        conversations: vec![
            DmConversation {
                peer_pubkey: "b".repeat(64),
                messages: vec![
                    sample_message("11", None, false),
                    sample_message("12", Some("11"), true),
                ],
            },
            DmConversation {
                peer_pubkey: "c".repeat(64),
                messages: vec![sample_message("21", None, false)],
            },
        ],
        decrypt_state: "ok".to_string(),
        undecrypted_count: 0,
    }
}

#[test]
fn round_trips_full_snapshot() {
    let snapshot = sample_snapshot();
    let bytes = encode_dm_inbox_snapshot(&snapshot);
    let decoded = decode_dm_inbox_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn round_trips_decrypt_state_and_empty_conversations() {
    // §D7 — the tri-state + count round-trip through the typed wire.
    let snapshot = DmInboxSnapshot {
        conversations: vec![],
        decrypt_state: "limited".to_string(),
        undecrypted_count: 7,
    };
    let bytes = encode_dm_inbox_snapshot(&snapshot);
    let decoded = decode_dm_inbox_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.decrypt_state, "limited");
    assert_eq!(decoded.undecrypted_count, 7);
    assert!(decoded.conversations.is_empty());
}

#[test]
fn unavailable_state_round_trips() {
    let snapshot = DmInboxSnapshot {
        conversations: vec![],
        decrypt_state: "unavailable".to_string(),
        undecrypted_count: 0,
    };
    let bytes = encode_dm_inbox_snapshot(&snapshot);
    let decoded = decode_dm_inbox_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn reply_to_none_round_trips_distinctly_from_present() {
    let snapshot = sample_snapshot();
    let bytes = encode_dm_inbox_snapshot(&snapshot);
    let decoded = decode_dm_inbox_snapshot(&bytes).expect("decode");
    // First message: reply_to None; second: Some.
    assert_eq!(decoded.conversations[0].messages[0].reply_to, None);
    assert_eq!(
        decoded.conversations[0].messages[1].reply_to.as_deref(),
        Some("11")
    );
}

#[test]
fn order_is_preserved() {
    let snapshot = sample_snapshot();
    let bytes = encode_dm_inbox_snapshot(&snapshot);
    let decoded = decode_dm_inbox_snapshot(&bytes).expect("decode");
    assert_eq!(decoded.conversations[0].peer_pubkey, "b".repeat(64));
    assert_eq!(decoded.conversations[1].peer_pubkey, "c".repeat(64));
    assert_eq!(decoded.conversations[0].messages[0].id, "11");
    assert_eq!(decoded.conversations[0].messages[1].id, "12");
    assert_eq!(
        decoded.conversations[0].messages[0].source_relays,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()]
    );
}

#[test]
fn buffer_carries_ndmi_identifier() {
    let bytes = encode_dm_inbox_snapshot(&sample_snapshot());
    assert_eq!(&bytes[4..8], DM_INBOX_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_garbage() {
    assert!(decode_dm_inbox_snapshot(&[0u8; 4]).is_err());
    assert!(decode_dm_inbox_snapshot(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_consts_are_stable() {
    assert_eq!(DM_INBOX_SCHEMA_ID, "nmp.nip17.dm_inbox");
    assert_eq!(DM_INBOX_FILE_IDENTIFIER, b"NDMI");
    assert_eq!(DM_INBOX_SCHEMA_VERSION, 2);
}
