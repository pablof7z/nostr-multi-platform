//! Round-trip + envelope tests for the `dm_relay_list` typed FlatBuffers codec.

use super::*;

#[test]
fn round_trips_signed_in() {
    let relay_list = DmRelayList {
        active_pubkey: Some("a".repeat(64)),
        read_relay_urls: vec![
            "wss://dm.relay.one".to_string(),
            "wss://dm.relay.two".to_string(),
        ],
    };
    let bytes = encode_dm_relay_list(&relay_list);
    let decoded = decode_dm_relay_list(&bytes).expect("decode");
    assert_eq!(decoded, relay_list);
}

#[test]
fn active_pubkey_none_round_trips_distinctly_from_empty() {
    let absent = DmRelayList {
        active_pubkey: None,
        read_relay_urls: vec![],
    };
    let bytes = encode_dm_relay_list(&absent);
    let decoded = decode_dm_relay_list(&bytes).expect("decode");
    assert_eq!(decoded.active_pubkey, None);
    assert!(decoded.read_relay_urls.is_empty());

    let present_empty = DmRelayList {
        active_pubkey: Some(String::new()),
        read_relay_urls: vec![],
    };
    let bytes = encode_dm_relay_list(&present_empty);
    let decoded = decode_dm_relay_list(&bytes).expect("decode");
    assert_eq!(decoded.active_pubkey, Some(String::new()));
}

#[test]
fn relay_url_order_preserved() {
    let relay_list = DmRelayList {
        active_pubkey: Some("a".repeat(64)),
        read_relay_urls: vec![
            "wss://z.relay".to_string(),
            "wss://a.relay".to_string(),
            "wss://m.relay".to_string(),
        ],
    };
    let bytes = encode_dm_relay_list(&relay_list);
    let decoded = decode_dm_relay_list(&bytes).expect("decode");
    assert_eq!(decoded.read_relay_urls, relay_list.read_relay_urls);
}

#[test]
fn buffer_carries_ndrl_identifier() {
    let bytes = encode_dm_relay_list(&DmRelayList::default());
    assert_eq!(&bytes[4..8], DM_RELAY_LIST_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_garbage() {
    assert!(decode_dm_relay_list(&[0u8; 4]).is_err());
    assert!(decode_dm_relay_list(b"not a flatbuffer").is_err());
}

#[test]
fn schema_consts_are_stable() {
    assert_eq!(DM_RELAY_LIST_SCHEMA_ID, "nmp.nip17.dm_relay_list");
    assert_eq!(DM_RELAY_LIST_FILE_IDENTIFIER, b"NDRL");
    assert_eq!(DM_RELAY_LIST_SCHEMA_VERSION, 1);
}
