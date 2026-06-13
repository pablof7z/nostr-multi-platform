//! Round-trip proof for the `claimed_profiles` Tier-2 typed codec.

use super::*;

fn card(pubkey: &str, named: bool) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: pubkey.to_string(),
        // ADR-0032 / V-115: `npub` deprecated; always empty after decode.
        npub: String::new(),
        display_name: named.then(|| "Alice".to_string()),
        picture_url: named.then(|| "https://example.com/a.png".to_string()),
        nip05: if named {
            "alice@example.com".to_string()
        } else {
            String::new()
        },
        about: if named {
            "hello".to_string()
        } else {
            String::new()
        },
        lnurl: named.then(|| "lnurl1abc".to_string()),
    }
}

fn sample() -> ClaimedProfilesModel {
    ClaimedProfilesModel {
        entries: vec![
            ("aa".repeat(32), card(&"aa".repeat(32), true)),
            // Placeholder card — claimed but no kind:0 yet.
            ("bb".repeat(32), card(&"bb".repeat(32), false)),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_claimed_profiles(&encode_claimed_profiles(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every card field and Option presence"
    );
}

#[test]
fn empty_map_round_trips() {
    let model = ClaimedProfilesModel::default();
    let decoded =
        decode_claimed_profiles(&encode_claimed_profiles(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.entries.is_empty());
}

#[test]
fn placeholder_card_keeps_none_options() {
    let model = ClaimedProfilesModel {
        entries: vec![("cc".repeat(32), card(&"cc".repeat(32), false))],
    };
    let decoded =
        decode_claimed_profiles(&encode_claimed_profiles(&model)).expect("decode succeeds");
    let got = &decoded.entries[0].1;
    assert_eq!(got.display_name, None);
    assert_eq!(got.picture_url, None);
    assert_eq!(got.lnurl, None);
}

#[test]
fn buffer_carries_the_kcpr_file_identifier() {
    let bytes = encode_claimed_profiles(&sample());
    assert_eq!(
        &bytes[4..8],
        CLAIMED_PROFILES_FILE_IDENTIFIER,
        "the buffer must embed the KCPR file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_claimed_profiles(&[]).is_err());
    assert!(decode_claimed_profiles(b"NMPU0000").is_err());
}
