//! Round-trip proof for the `mention_profiles` Tier-2 typed codec.

use super::*;

fn sample() -> MentionProfilesModel {
    MentionProfilesModel {
        entries: vec![
            (
                "aa".repeat(32),
                MentionProfileRow {
                    pubkey: "aa".repeat(32),
                    display_name: Some("Alice".to_string()),
                    picture_url: Some("https://example.com/a.png".to_string()),
                },
            ),
            (
                "bb".repeat(32),
                MentionProfileRow {
                    pubkey: "bb".repeat(32),
                    // No kind:0 yet for this author — both Options are None.
                    display_name: None,
                    picture_url: None,
                },
            ),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_mention_profiles(&encode_mention_profiles(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every entry, key order, and Option presence"
    );
}

#[test]
fn empty_map_round_trips() {
    let model = MentionProfilesModel::default();
    let decoded =
        decode_mention_profiles(&encode_mention_profiles(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.entries.is_empty());
}

#[test]
fn none_options_distinct_from_empty_string() {
    // `display_name: None` (no kind:0) must decode back to None, not "".
    let model = MentionProfilesModel {
        entries: vec![(
            "cc".repeat(32),
            MentionProfileRow {
                pubkey: "cc".repeat(32),
                display_name: None,
                picture_url: Some(String::new()),
            },
        )],
    };
    let decoded =
        decode_mention_profiles(&encode_mention_profiles(&model)).expect("decode succeeds");
    assert_eq!(decoded.entries[0].1.display_name, None);
    assert_eq!(decoded.entries[0].1.picture_url, Some(String::new()));
}

#[test]
fn buffer_carries_the_kmpr_file_identifier() {
    let bytes = encode_mention_profiles(&sample());
    assert_eq!(
        &bytes[4..8],
        MENTION_PROFILES_FILE_IDENTIFIER,
        "the buffer must embed the KMPR file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_mention_profiles(&[]).is_err());
    assert!(decode_mention_profiles(b"NMPU0000").is_err());
}
