//! Round-trip proof for the `accounts` Tier-2 typed codec.

use super::*;

fn sample() -> AccountsModel {
    AccountsModel {
        accounts: vec![
            // Fully-populated active row (both Options present).
            AccountSummaryRow {
                id: "a".repeat(64),
                npub: "npub1aaa".to_string(),
                display_name: Some("Alice".to_string()),
                signer_kind: "local".to_string(),
                status: "active".to_string(),
                signer_label: "nsec".to_string(),
                signer_is_remote: false,
                is_active: true,
                picture_url: Some("https://img/alice.png".to_string()),
            },
            // Idle remote row with both Options absent (None).
            AccountSummaryRow {
                id: "b".repeat(64),
                npub: "npub1bbb".to_string(),
                display_name: None,
                signer_kind: "nip46".to_string(),
                status: "idle".to_string(),
                signer_label: "NIP-46".to_string(),
                signer_is_remote: true,
                is_active: false,
                picture_url: None,
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips_and_preserves_order_and_options() {
    let model = sample();
    let bytes = encode_accounts(&model);
    let decoded = decode_accounts(&bytes).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every row, in order, incl. None vs Some"
    );
    // Spot-check the option presence flags survive distinctly.
    assert_eq!(decoded.accounts[0].display_name.as_deref(), Some("Alice"));
    assert!(decoded.accounts[1].display_name.is_none());
    assert!(decoded.accounts[0].picture_url.is_some());
    assert!(decoded.accounts[1].picture_url.is_none());
}

#[test]
fn empty_accounts_round_trips() {
    let model = AccountsModel::default();
    let bytes = encode_accounts(&model);
    let decoded = decode_accounts(&bytes).expect("decode must succeed");
    assert!(decoded.accounts.is_empty());
}

#[test]
fn buffer_carries_the_kacc_file_identifier() {
    let bytes = encode_accounts(&sample());
    assert_eq!(
        &bytes[4..8],
        ACCOUNTS_FILE_IDENTIFIER,
        "the buffer must embed the KACC file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_accounts(&[]).is_err());
    assert!(decode_accounts(b"NMPU0000").is_err());
}
