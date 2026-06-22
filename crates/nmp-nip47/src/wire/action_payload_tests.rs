//! Round-trip + fail-closed tests for the three wallet action-payload codecs
//! (ADR-0064 / #1756). Every fail-closed gate asserts the NEGATIVE, and the
//! `Option<u64>` presence on `pay_invoice` is exercised in both states (the
//! `Some(0)` vs `None` distinction the companion `has_amount_msats` flag
//! protects).

use super::*;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

// --- nmp.wallet.connect ------------------------------------------------------

#[test]
fn connect_round_trips() {
    let action = WalletConnectAction::Connect {
        uri: "nostr+walletconnect://abc123?relay=wss://relay.example&secret=xyz".to_string(),
    };
    let decoded = WalletConnectAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn connect_empty_uri_round_trips() {
    // The codec is a pure shape decode; non-empty validation lives in start().
    // An empty uri must round-trip faithfully rather than be rejected here.
    let action = WalletConnectAction::Connect {
        uri: String::new(),
    };
    let decoded = WalletConnectAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn connect_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let uri = fbb.create_string("nostr+walletconnect://abc");
    let payload = connect_fb::WalletConnectPayload::create(
        &mut fbb,
        &connect_fb::WalletConnectPayloadArgs {
            schema_version: 999,
            uri: Some(uri),
        },
    );
    connect_fb::finish_wallet_connect_payload_buffer(&mut fbb, payload);
    let err = WalletConnectAction::decode(fbb.finished_data()).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn connect_malformed_buffers_are_rejected() {
    assert!(matches!(
        WalletConnectAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        WalletConnectAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn connect_wrong_identifier_is_rejected() {
    let action = WalletConnectAction::Connect {
        uri: "nostr+walletconnect://abc".to_string(),
    };
    let mut bytes = action.encode();
    // The file identifier sits at bytes[4..8].
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        WalletConnectAction::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

// --- nmp.wallet.disconnect ---------------------------------------------------

#[test]
fn disconnect_round_trips() {
    let action = WalletDisconnectAction::Disconnect;
    let decoded = WalletDisconnectAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn disconnect_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let payload = disconnect_fb::WalletDisconnectPayload::create(
        &mut fbb,
        &disconnect_fb::WalletDisconnectPayloadArgs {
            schema_version: 999,
        },
    );
    disconnect_fb::finish_wallet_disconnect_payload_buffer(&mut fbb, payload);
    let err =
        WalletDisconnectAction::decode(fbb.finished_data()).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn disconnect_malformed_buffers_are_rejected() {
    assert!(matches!(
        WalletDisconnectAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        WalletDisconnectAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn disconnect_wrong_identifier_is_rejected() {
    let mut bytes = WalletDisconnectAction::Disconnect.encode();
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        WalletDisconnectAction::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

// --- nmp.wallet.pay_invoice --------------------------------------------------

fn pay_invoice_round_trip(amount_msats: Option<u64>) {
    let action = WalletAction::PayInvoice {
        bolt11: "lnbc100n1p0roundtrip".to_string(),
        amount_msats,
    };
    let decoded = WalletAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn pay_invoice_round_trips_without_amount() {
    pay_invoice_round_trip(None);
}

#[test]
fn pay_invoice_round_trips_with_amount() {
    pay_invoice_round_trip(Some(21_000));
}

#[test]
fn pay_invoice_zero_amount_presence_is_preserved_not_collapsed() {
    // The presence-flag lesson: Some(0) must NOT collapse to None, and None must
    // NOT read as Some(0). Both endpoints of the companion `has_amount_msats`
    // flag are pinned here.
    let some_zero = WalletAction::PayInvoice {
        bolt11: "lnbc1p0amountless".to_string(),
        amount_msats: Some(0),
    };
    let decoded = WalletAction::decode(&some_zero.encode()).expect("decodes");
    match decoded {
        WalletAction::PayInvoice { amount_msats, .. } => {
            assert_eq!(amount_msats, Some(0), "Some(0) must stay Some(0)");
        }
    }

    let none = WalletAction::PayInvoice {
        bolt11: "lnbc1p0amountless".to_string(),
        amount_msats: None,
    };
    let decoded = WalletAction::decode(&none.encode()).expect("decodes");
    match decoded {
        WalletAction::PayInvoice { amount_msats, .. } => {
            assert_eq!(amount_msats, None, "None must stay None (not Some(0))");
        }
    }
}

#[test]
fn pay_invoice_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let bolt11 = fbb.create_string("lnbc100n1p0fake");
    let payload = pay_invoice_fb::WalletPayInvoicePayload::create(
        &mut fbb,
        &pay_invoice_fb::WalletPayInvoicePayloadArgs {
            schema_version: 999,
            bolt11: Some(bolt11),
            amount_msats: 0,
            has_amount_msats: false,
        },
    );
    pay_invoice_fb::finish_wallet_pay_invoice_payload_buffer(&mut fbb, payload);
    let err = WalletAction::decode(fbb.finished_data()).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn pay_invoice_malformed_buffers_are_rejected() {
    assert!(matches!(
        WalletAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        WalletAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn pay_invoice_wrong_identifier_is_rejected() {
    let action = WalletAction::PayInvoice {
        bolt11: "lnbc100n1p0fake".to_string(),
        amount_msats: None,
    };
    let mut bytes = action.encode();
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        WalletAction::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
