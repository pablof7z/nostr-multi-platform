//! `sign_zap_request` — round-trip, kind range, and the V-78 byte-identity
//! guarantee (`signed_event_to_nostr_json` reproduces the same flat NIP-01
//! wire bytes whether the kind:9734 was signed locally or via the port).

use super::*;

#[test]
fn sign_zap_request_round_trips_through_event_builder() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec!["relays".to_string(), "wss://relay.example".to_string()],
            vec![
                "p".to_string(),
                "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff".to_string(),
            ],
        ],
        content: "great post 🤙".to_string(),
        created_at: 1_700_000_000,
    };
    let json = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    let event: nostr::Event =
        serde_json::from_str(&json).expect("signed output must be a valid nostr::Event");
    assert_eq!(event.kind.as_u16(), 9734);
    assert_eq!(event.content, "great post 🤙");
    assert!(!event.sig.to_string().is_empty());
}

#[test]
fn sign_zap_request_rejects_out_of_range_kind() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        // 100_000 is outside the u16 range nostr::Kind accepts.
        kind: 100_000,
        tags: Vec::new(),
        content: String::new(),
        created_at: 0,
    };
    assert!(sign_zap_request(&keys, &unsigned).is_err());
}

/// V-78 — `signed_event_to_nostr_json` must reproduce the EXACT flat NIP-01
/// wire bytes `sign_zap_request` emits, so a bunker-signed kind:9734 hits the
/// LN provider's callback byte-for-byte identical to a local-nsec zap. The
/// signed `nostr::Event` is flattened to `SignedEvent` and rebuilt; the two
/// serializations must be equal.
#[test]
fn signed_event_to_nostr_json_matches_sign_zap_request_bytes() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec!["relays".to_string(), "wss://relay.example".to_string()],
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
            vec!["amount".to_string(), "21000".to_string()],
        ],
        content: "nice post 🤙".to_string(),
        created_at: 1_700_000_000,
    };

    // The canonical local path.
    let direct = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    // Flatten that signed event into a substrate SignedEvent, then rebuild
    // the flat JSON through the V-78 helper.
    let event: nostr::Event = serde_json::from_str(&direct).expect("valid event");
    let signed = nostr_event_to_signed(&event);
    let rebuilt = signed_event_to_nostr_json(&signed).expect("rebuild must succeed");

    assert_eq!(
        direct, rebuilt,
        "the bunker-rebuilt flat NIP-01 JSON must be byte-identical to the \
         local-nsec sign output"
    );
}
