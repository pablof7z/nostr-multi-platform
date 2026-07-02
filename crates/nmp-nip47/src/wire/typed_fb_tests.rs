//! Round-trip tests for the [`WalletStatus`] typed FlatBuffers codec.

use super::{
    decode_wallet_status, encode_wallet_status, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
use crate::status::{NwcConnectionState, WalletStatus};

fn full_status() -> WalletStatus {
    WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://relay.example/nwc".to_string(),
        wallet_npub: "npub1walletservicepubkeybech32".to_string(),
        wallet_pubkey_hex: "ab".repeat(32),
        balance_msats: Some(12_345_000),
        balance_sats: Some(12_345),
        // `wallet_npub_short` removed (#1678, D7) — shells abbreviate.
        is_ready: true,
        is_connected: true,
        connection_state: Some(NwcConnectionState::Connected),
    }
}

#[test]
fn round_trips_fully_populated_status() {
    let status = full_status();
    let bytes = encode_wallet_status(&status);
    let decoded = decode_wallet_status(&bytes).expect("decode must succeed");
    assert_eq!(decoded, status);
}

#[test]
fn round_trips_disconnected_status_with_all_options_none() {
    // The pre-balance / disconnected shape: every `Option` is `None`.
    let status = WalletStatus {
        status: "disconnected".to_string(),
        relay_url: String::new(),
        wallet_npub: String::new(),
        wallet_pubkey_hex: String::new(),
        balance_msats: None,
        balance_sats: None,
        // `wallet_npub_short` removed (#1678, D7).
        is_ready: false,
        is_connected: false,
        connection_state: None,
    };
    let bytes = encode_wallet_status(&status);
    let decoded = decode_wallet_status(&bytes).expect("decode must succeed");
    assert_eq!(decoded, status);
    assert!(decoded.balance_msats.is_none());
    assert!(decoded.connection_state.is_none());
}

#[test]
fn each_connection_state_variant_round_trips() {
    for variant in [
        NwcConnectionState::Connected,
        NwcConnectionState::Reconnecting,
        NwcConnectionState::TransportLost,
    ] {
        let mut status = full_status();
        status.connection_state = Some(variant.clone());
        let bytes = encode_wallet_status(&status);
        let decoded = decode_wallet_status(&bytes).expect("decode must succeed");
        assert_eq!(decoded.connection_state, Some(variant));
    }
}

#[test]
fn encoded_buffer_carries_the_nwst_file_identifier() {
    let bytes = encode_wallet_status(&full_status());
    assert!(super::generated::nmp::nip_47::wallet_status_buffer_has_identifier(&bytes));
    assert_eq!(FILE_IDENTIFIER, b"NWST");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    assert!(decode_wallet_status(&[]).is_err());
    assert!(decode_wallet_status(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_constants_match_the_fbs() {
    assert_eq!(SCHEMA_ID, "nmp.nip47.wallet");
    assert_eq!(SCHEMA_VERSION, 1);
}

// Raw-data doctrine (aim.md §2 / ADR-0072): the wire carries the raw `status`
// token verbatim — shells map it to a label/tone themselves. The deleted
// `status_label` / `status_tone` / `balance_sats_display` precompute fields are
// gone (wallet_status sweep). This proves the raw token survives the round-trip
// for every wire status the shells branch on.
#[test]
fn raw_status_token_round_trips_for_every_wire_state() {
    for wire in ["connecting", "ready", "error", "disconnected"] {
        let mut status = full_status();
        status.status = wire.to_string();
        let bytes = encode_wallet_status(&status);
        let decoded = decode_wallet_status(&bytes).expect("decode must succeed");
        assert_eq!(decoded.status, wire);
    }
}
