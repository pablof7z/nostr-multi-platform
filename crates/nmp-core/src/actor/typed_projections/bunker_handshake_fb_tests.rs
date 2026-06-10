use super::{decode_bunker_handshake, encode_bunker_handshake, BunkerHandshakeModel};

#[test]
fn round_trips_full_model() {
    let model = BunkerHandshakeModel {
        stage: "connecting".to_string(),
        message: Some("wss://relay.example".to_string()),
        is_idle: false,
        is_in_flight: true,
        is_failed: false,
        is_terminal_success: false,
        can_cancel: true,
        stage_label: "Connecting to bunker relays…".to_string(),
    };
    let bytes = encode_bunker_handshake(&model);
    let decoded = decode_bunker_handshake(&bytes).expect("decodes");
    assert_eq!(decoded, model);
}

#[test]
fn message_none_round_trips_as_none() {
    let model = BunkerHandshakeModel {
        stage: "failed".to_string(),
        message: None,
        is_idle: false,
        is_in_flight: false,
        is_failed: true,
        is_terminal_success: false,
        can_cancel: false,
        stage_label: "Bunker handshake failed".to_string(),
    };
    let bytes = encode_bunker_handshake(&model);
    let decoded = decode_bunker_handshake(&bytes).expect("decodes");
    assert_eq!(decoded.message, None);
    assert_eq!(decoded, model);
}

#[test]
fn rejects_foreign_identifier() {
    // A buffer without the KBHS identifier must fail closed (D6: no panic).
    assert!(decode_bunker_handshake(b"not a flatbuffer").is_err());
    assert!(decode_bunker_handshake(&[]).is_err());
}
