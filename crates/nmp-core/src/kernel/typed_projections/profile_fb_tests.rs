//! Round-trip proof for the `profile` Tier-2 typed codec.

use super::*;

fn populated() -> ProfileCardModel {
    ProfileCardModel {
        pubkey: "a".repeat(64),
        // ADR-0032 / V-115: `npub` deprecated; always empty in codec round-trips.
        npub: String::new(),
        display_name: Some("Alice".to_string()),
        picture_url: Some("https://img/alice.png".to_string()),
        nip05: "alice@example.com".to_string(),
        about: "hello".to_string(),
        lnurl: Some("alice@walletofsatoshi.com".to_string()),
    }
}

fn placeholder() -> ProfileCardModel {
    // No kind:0 yet — every Option is None; non-Option strings stay present.
    ProfileCardModel {
        pubkey: String::new(),
        npub: String::new(),
        display_name: None,
        picture_url: None,
        nip05: String::new(),
        about: "Waiting for kind:0 from indexer".to_string(),
        lnurl: None,
    }
}

#[test]
fn populated_card_round_trips() {
    let model = populated();
    let bytes = encode_profile(&model);
    let decoded = decode_profile(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
}

#[test]
fn placeholder_card_round_trips_with_all_options_none() {
    let model = placeholder();
    let bytes = encode_profile(&model);
    let decoded = decode_profile(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.display_name.is_none());
    assert!(decoded.picture_url.is_none());
    assert!(decoded.lnurl.is_none());
}

#[test]
fn buffer_carries_the_kprf_file_identifier() {
    let bytes = encode_profile(&populated());
    assert_eq!(
        &bytes[4..8],
        PROFILE_FILE_IDENTIFIER,
        "the buffer must embed the KPRF file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_profile(&[]).is_err());
    assert!(decode_profile(b"NMPU0000").is_err());
}
