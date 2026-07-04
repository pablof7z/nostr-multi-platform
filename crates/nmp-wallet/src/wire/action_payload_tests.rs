//! Round-trip + fail-closed tests for the nine `nmp.wallet.*` typed payload
//! codecs (#2920, epic #2864; `set_mints` added by #2997). Every fail-closed
//! gate asserts the NEGATIVE.

use super::*;
use crate::action::{
    CashuCompleteDepositAction, CashuCreateAction, CashuDepositQuoteAction, CashuRecoverAction,
    CashuSetMintsAction, NutzapPublishInfoAction, NutzapRedeemAction, NutzapSendAction,
    SelectBackendAction,
};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

// --- select_backend -----------------------------------------------------------

#[test]
fn select_backend_round_trips() {
    let action = SelectBackendAction {
        backend_id: "cashu".to_string(),
    };
    let decoded = SelectBackendAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn select_backend_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let backend_id = fbb.create_string("nwc");
    let payload = select_backend_generated::nmp::wallet::SelectBackendPayload::create(
        &mut fbb,
        &select_backend_generated::nmp::wallet::SelectBackendPayloadArgs {
            schema_version: 999,
            backend_id: Some(backend_id),
        },
    );
    select_backend_generated::nmp::wallet::finish_select_backend_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = SelectBackendAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

// --- cashu.create ---------------------------------------------------------------

#[test]
fn cashu_create_round_trips() {
    let action = CashuCreateAction {
        mint: "https://mint.example.com".to_string(),
    };
    let decoded = CashuCreateAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- cashu.recover ----------------------------------------------------------------

#[test]
fn cashu_recover_round_trips() {
    let action = CashuRecoverAction {};
    let decoded = CashuRecoverAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- cashu.deposit_quote -----------------------------------------------------------

#[test]
fn cashu_deposit_quote_round_trips() {
    let action = CashuDepositQuoteAction {
        mint: "https://mint.example.com".to_string(),
        amount_sats: 21_000,
    };
    let decoded = CashuDepositQuoteAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- cashu.complete_deposit --------------------------------------------------------

#[test]
fn cashu_complete_deposit_round_trips() {
    let action = CashuCompleteDepositAction {
        quote_id: "quote-abc-123".to_string(),
    };
    let decoded = CashuCompleteDepositAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- cashu.set_mints ---------------------------------------------------------------

#[test]
fn cashu_set_mints_round_trips() {
    let action = CashuSetMintsAction {
        mints: vec![
            "https://mint-a.example".to_string(),
            "https://mint-b.example".to_string(),
        ],
    };
    let decoded = CashuSetMintsAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn cashu_set_mints_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let mint_off = fbb.create_string("https://mint.example.com");
    let mints = fbb.create_vector(&[mint_off]);
    let payload = cashu_set_mints_generated::nmp::wallet::CashuSetMintsPayload::create(
        &mut fbb,
        &cashu_set_mints_generated::nmp::wallet::CashuSetMintsPayloadArgs {
            schema_version: 999,
            mints: Some(mints),
        },
    );
    cashu_set_mints_generated::nmp::wallet::finish_cashu_set_mints_payload_buffer(
        &mut fbb, payload,
    );
    let bytes = fbb.finished_data().to_vec();
    let err = CashuSetMintsAction::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

// --- nutzap.publish_info -----------------------------------------------------------

#[test]
fn nutzap_publish_info_round_trips() {
    let action = NutzapPublishInfoAction {};
    let decoded = NutzapPublishInfoAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- nutzap.send --------------------------------------------------------------------

#[test]
fn nutzap_send_round_trips_with_target_event() {
    let action = NutzapSendAction {
        recipient_pubkey: "a".repeat(64),
        amount_sats: 100,
        target_event_id: Some("b".repeat(64)),
    };
    let decoded = NutzapSendAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn nutzap_send_round_trips_without_target_event() {
    let action = NutzapSendAction {
        recipient_pubkey: "a".repeat(64),
        amount_sats: 100,
        target_event_id: None,
    };
    let decoded = NutzapSendAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.target_event_id.is_none());
}

// --- nutzap.redeem ------------------------------------------------------------------

#[test]
fn nutzap_redeem_round_trips() {
    let action = NutzapRedeemAction {
        event_id: "c".repeat(64),
    };
    let decoded = NutzapRedeemAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// --- fail-closed: malformed buffers, every namespace -------------------------------

#[test]
fn malformed_buffers_are_rejected_for_every_namespace() {
    assert!(matches!(
        SelectBackendAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CashuCreateAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CashuRecoverAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CashuDepositQuoteAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CashuCompleteDepositAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        CashuSetMintsAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        NutzapPublishInfoAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        NutzapSendAction::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        NutzapRedeemAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
