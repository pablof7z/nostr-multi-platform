//! Round-trip proof for the `resolved_profiles` Tier-2 typed codec.

use super::*;

fn card(pubkey: &str, named: bool) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: pubkey.to_string(),
        // ADR-0032 / V-115: `npub` deprecated; always empty after decode.
        npub: String::new(),
        display_name: named.then(|| "Bob".to_string()),
        picture_url: named.then(|| "https://example.com/b.png".to_string()),
        nip05: if named {
            "bob@example.com".to_string()
        } else {
            String::new()
        },
        about: if named {
            "world".to_string()
        } else {
            String::new()
        },
        lnurl: named.then(|| "lnurl1def".to_string()),
    }
}

fn sample() -> ResolvedProfilesModel {
    ResolvedProfilesModel {
        entries: vec![
            ("aa".repeat(32), card(&"aa".repeat(32), true)),
            ("bb".repeat(32), card(&"bb".repeat(32), false)),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_resolved_profiles(&encode_resolved_profiles(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every card field and Option presence"
    );
}

#[test]
fn empty_map_round_trips() {
    let model = ResolvedProfilesModel::default();
    let decoded =
        decode_resolved_profiles(&encode_resolved_profiles(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.entries.is_empty());
}

#[test]
fn buffer_carries_the_krpr_file_identifier() {
    let bytes = encode_resolved_profiles(&sample());
    assert_eq!(
        &bytes[4..8],
        RESOLVED_PROFILES_FILE_IDENTIFIER,
        "the buffer must embed the KRPR file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_resolved_profiles(&[]).is_err());
    assert!(decode_resolved_profiles(b"NMPU0000").is_err());
}
