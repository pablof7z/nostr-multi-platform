//! Round-trip proof for the `settings_hub` Tier-2 typed codec.

use super::*;

#[test]
fn encode_decode_round_trips() {
    let model = SettingsHubModel { relay_count: 7 };
    let bytes = encode_settings_hub(&model);
    let decoded = decode_settings_hub(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
}

#[test]
fn zero_relay_count_round_trips() {
    let model = SettingsHubModel::default();
    let bytes = encode_settings_hub(&model);
    let decoded = decode_settings_hub(&bytes).expect("decode must succeed");
    assert_eq!(decoded.relay_count, 0);
}

#[test]
fn buffer_carries_the_kshb_file_identifier() {
    let bytes = encode_settings_hub(&SettingsHubModel { relay_count: 3 });
    assert_eq!(&bytes[4..8], SETTINGS_HUB_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_settings_hub(&[]).is_err());
    assert!(decode_settings_hub(b"NMPU0000").is_err());
}
