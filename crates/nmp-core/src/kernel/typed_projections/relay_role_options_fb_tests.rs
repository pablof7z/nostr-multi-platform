//! Round-trip proof for the `relay_role_options` Tier-2 typed codec.

use super::*;

fn sample() -> RelayRoleOptionsModel {
    RelayRoleOptionsModel {
        options: vec![
            RelayRoleOptionRow {
                value: "both".to_string(),
                label: "Both".to_string(),
                tint: "accent".to_string(),
                is_default: true,
            },
            RelayRoleOptionRow {
                value: "indexer".to_string(),
                label: "Index".to_string(),
                tint: "neutral".to_string(),
                is_default: false,
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips_and_preserves_order() {
    let model = sample();
    let bytes = encode_relay_role_options(&model);
    let decoded = decode_relay_role_options(&bytes).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every option, in order"
    );
}

#[test]
fn empty_options_round_trips() {
    let model = RelayRoleOptionsModel::default();
    let bytes = encode_relay_role_options(&model);
    let decoded = decode_relay_role_options(&bytes).expect("decode must succeed");
    assert!(decoded.options.is_empty());
}

#[test]
fn buffer_carries_the_krro_file_identifier() {
    let bytes = encode_relay_role_options(&sample());
    assert_eq!(&bytes[4..8], RELAY_ROLE_OPTIONS_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_relay_role_options(&[]).is_err());
    assert!(decode_relay_role_options(b"NMPU0000").is_err());
}

/// The kernel's `relay_role_options()` source round-trips through the codec
/// field-for-field. (The mapping itself lives inline in
/// `Kernel::builtin_typed_projections`; this asserts the codec carries every
/// field the source produces.)
#[test]
fn kernel_role_options_round_trip_field_for_field() {
    let options = crate::actor::relay_role_options();
    let model = RelayRoleOptionsModel {
        options: options
            .iter()
            .map(|option| RelayRoleOptionRow {
                value: option.value.clone(),
                label: option.label.clone(),
                tint: option.tint.clone(),
                is_default: option.is_default,
            })
            .collect(),
    };
    let decoded = decode_relay_role_options(&encode_relay_role_options(&model))
        .expect("decode must succeed");
    assert_eq!(decoded, model);
    assert_eq!(decoded.options.len(), options.len());
}
