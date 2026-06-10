//! Round-trip proof for the `active_account` Tier-2 typed codec.

use super::*;

#[test]
fn some_pubkey_round_trips() {
    let model = ActiveAccountModel {
        pubkey: Some("f".repeat(64)),
    };
    let bytes = encode_active_account(&model);
    let decoded = decode_active_account(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.pubkey.is_some());
}

#[test]
fn none_round_trips_as_absent() {
    // No active account ⇒ `has_active_account = false` ⇒ mirrors JSON `null`.
    let model = ActiveAccountModel::default();
    let bytes = encode_active_account(&model);
    let decoded = decode_active_account(&bytes).expect("decode must succeed");
    assert!(
        decoded.pubkey.is_none(),
        "absent active account must decode back to None, not Some(\"\")"
    );
}

#[test]
fn buffer_carries_the_kact_file_identifier() {
    let bytes = encode_active_account(&ActiveAccountModel {
        pubkey: Some("a".repeat(64)),
    });
    assert_eq!(
        &bytes[4..8],
        ACTIVE_ACCOUNT_FILE_IDENTIFIER,
        "the buffer must embed the KACT file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_active_account(&[]).is_err());
    assert!(decode_active_account(b"NMPU0000").is_err());
}
