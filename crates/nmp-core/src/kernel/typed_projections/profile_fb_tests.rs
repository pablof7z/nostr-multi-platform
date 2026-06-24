//! Round-trip proof for the `profile` Tier-2 typed codec.

use super::*;

fn populated() -> ProfileCardModel {
    ProfileCardModel {
        pubkey: "a".repeat(64),
        display_name: Some("Alice".to_string()),
        name: Some("alice".to_string()),
        raw_display_name: Some("Alice".to_string()),
        display_name_camel: Some("Alice Camel".to_string()),
        picture_url: Some("https://img/alice.png".to_string()),
        banner: Some("https://img/banner.png".to_string()),
        website: Some("https://alice.example".to_string()),
        nip05: "alice@example.com".to_string(),
        about: "hello".to_string(),
        lud16: Some("alice@ln.example".to_string()),
        lud06: Some("lnurl1abc".to_string()),
        lnurl: Some("alice@walletofsatoshi.com".to_string()),
    }
}

fn placeholder() -> ProfileCardModel {
    // No kind:0 yet — every Option is None; non-Option strings stay present.
    ProfileCardModel {
        pubkey: String::new(),
        display_name: None,
        name: None,
        raw_display_name: None,
        display_name_camel: None,
        picture_url: None,
        banner: None,
        website: None,
        nip05: String::new(),
        about: "Waiting for kind:0 from indexer".to_string(),
        lud16: None,
        lud06: None,
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
    assert!(decoded.name.is_none());
    assert!(decoded.raw_display_name.is_none());
    assert!(decoded.display_name_camel.is_none());
    assert!(decoded.picture_url.is_none());
    assert!(decoded.banner.is_none());
    assert!(decoded.website.is_none());
    assert!(decoded.lud16.is_none());
    assert!(decoded.lud06.is_none());
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
