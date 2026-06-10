//! Round-trip proof for the `configured_relays` Tier-2 typed codec.

use super::*;

fn sample() -> ConfiguredRelaysModel {
    ConfiguredRelaysModel {
        relays: vec![
            ConfiguredRelayRow {
                url: "wss://relay.one/".to_string(),
                role: "both".to_string(),
            },
            ConfiguredRelayRow {
                url: "wss://relay.two/".to_string(),
                role: "read,indexer".to_string(),
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips_and_preserves_order() {
    let model = sample();
    let bytes = encode_configured_relays(&model);
    let decoded = decode_configured_relays(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model, "round-trip must preserve every row, in order");
}

#[test]
fn empty_relays_round_trips() {
    let model = ConfiguredRelaysModel::default();
    let bytes = encode_configured_relays(&model);
    let decoded = decode_configured_relays(&bytes).expect("decode must succeed");
    assert!(decoded.relays.is_empty());
}

#[test]
fn buffer_carries_the_kcrl_file_identifier() {
    let bytes = encode_configured_relays(&sample());
    assert_eq!(
        &bytes[4..8],
        CONFIGURED_RELAYS_FILE_IDENTIFIER,
        "the buffer must embed the KCRL file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_configured_relays(&[]).is_err());
    assert!(decode_configured_relays(b"NMPU0000").is_err());
}

#[test]
fn from_app_relay_slice_mirrors_url_and_role() {
    let rows = [
        crate::kernel::AppRelay::new("wss://a/".to_string(), "both".to_string()),
        crate::kernel::AppRelay::new("wss://b/".to_string(), "read".to_string()),
    ];
    let model = ConfiguredRelaysModel::from(rows.as_slice());
    assert_eq!(model.relays.len(), 2);
    assert_eq!(model.relays[0].url, "wss://a/");
    assert_eq!(model.relays[0].role, "both");
    assert_eq!(model.relays[1].role, "read");
}
