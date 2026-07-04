//! Round-trip tests for the merged [`WalletProjection`] typed FlatBuffers codec
//! (#2915). Mirrors `nmp-nip47::wire::typed_fb_tests` / `nmp-wot::wire::typed_fb`:
//! encode a `WalletProjection`, decode it back, assert structural equality.

use super::{
    decode_wallet_projection, encode_wallet_projection, FILE_IDENTIFIER, PROJECTION_KEY, SCHEMA_ID,
    SCHEMA_VERSION,
};
use crate::backend::WalletBackendId;
use crate::capability::WalletCapabilities;
use crate::journal::{
    WalletConsumedInput, WalletOperation, WalletOperationId, WalletOperationKind,
    WalletOperationState,
};
use crate::projection::{
    WalletBalanceRow, WalletHistoryKind, WalletHistoryRow, WalletProjection, WalletReadiness,
    WalletReceiveRow,
};

fn fully_populated() -> WalletProjection {
    let op = WalletOperation {
        id: WalletOperationId::new("op-1"),
        kind: WalletOperationKind::SendNutzap,
        state: WalletOperationState::MintPending,
        correlation_id: Some("corr-1".to_string()),
        consumed_inputs: vec![
            WalletConsumedInput {
                event_id: "ev-a".to_string(),
                mint: "https://mint.example".to_string(),
                unit: "sat".to_string(),
                amount: 21,
            },
            WalletConsumedInput {
                event_id: "ev-b".to_string(),
                mint: "https://mint.example".to_string(),
                unit: "sat".to_string(),
                amount: 42,
            },
        ],
        recorded_amount: Some(63),
        recorded_sender: Some("cc".repeat(32)),
        recorded_at: Some(1_700_000_000),
    };

    WalletProjection {
        active_backend_id: Some(WalletBackendId::new("cashu")),
        readiness: WalletReadiness::Ready,
        capabilities: WalletCapabilities::cashu_nutzaps(),
        balances: vec![
            WalletBalanceRow {
                mint: "https://mint.example".to_string(),
                unit: "sat".to_string(),
                amount: 1_000,
            },
            WalletBalanceRow {
                mint: "https://mint.two".to_string(),
                unit: "usd".to_string(),
                amount: 7,
            },
        ],
        cashu_p2pk_pubkey: Some("ab".repeat(32)),
        accepted_mint_count: 2,
        accepted_relay_count: 3,
        pending_operations: vec![op],
        recent_history: vec![WalletHistoryRow {
            operation_id: "op-0".to_string(),
            kind: WalletHistoryKind::Deposit,
            amount: 500,
            unit: "sat".to_string(),
            sender: Some("dd".repeat(32)),
            timestamp: Some(1_700_000_100),
            state: "settled".to_string(),
        }],
        receive_rows: vec![WalletReceiveRow {
            event_id: "recv-1".to_string(),
            mint: "https://mint.example".to_string(),
            amount: 64,
            unit: "sat".to_string(),
            sender: Some("ee".repeat(32)),
            timestamp: Some(1_700_000_200),
            accepted: true,
        }],
    }
}

#[test]
fn round_trips_fully_populated_projection() {
    let projection = fully_populated();
    let bytes = encode_wallet_projection(&projection);
    let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded, projection);
}

#[test]
fn round_trips_empty_projection_with_all_options_none() {
    let projection = WalletProjection::empty();
    let bytes = encode_wallet_projection(&projection);
    let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded, projection);
    assert!(decoded.active_backend_id.is_none());
    assert!(decoded.cashu_p2pk_pubkey.is_none());
    assert!(decoded.balances.is_empty());
    assert!(decoded.pending_operations.is_empty());
    assert!(decoded.recent_history.is_empty());
    assert!(decoded.receive_rows.is_empty());
}

#[test]
fn round_trips_operation_with_no_correlation_id_or_inputs() {
    let projection = WalletProjection::empty().with_pending_operations([WalletOperation::new(
        WalletOperationId::new("op-bare"),
        WalletOperationKind::PayBolt11,
        WalletOperationState::Draft,
    )]);
    let bytes = encode_wallet_projection(&projection);
    let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded, projection);
    let op = &decoded.pending_operations[0];
    assert!(op.correlation_id.is_none());
    assert!(op.consumed_inputs.is_empty());
    assert!(op.recorded_amount.is_none());
    assert!(op.recorded_sender.is_none());
    assert!(op.recorded_at.is_none());
}

#[test]
fn each_readiness_variant_round_trips() {
    for variant in [
        WalletReadiness::NotConfigured,
        WalletReadiness::Activating,
        WalletReadiness::Ready,
        WalletReadiness::Degraded,
    ] {
        let mut projection = fully_populated();
        projection.readiness = variant;
        let bytes = encode_wallet_projection(&projection);
        let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
        assert_eq!(decoded.readiness, variant);
    }
}

#[test]
fn each_history_kind_variant_round_trips() {
    for variant in [
        WalletHistoryKind::Deposit,
        WalletHistoryKind::SendNutzap,
        WalletHistoryKind::RedeemNutzap,
        WalletHistoryKind::PayBolt11,
    ] {
        let projection = WalletProjection::empty().with_recent_history([WalletHistoryRow {
            operation_id: "op".to_string(),
            kind: variant,
            amount: 1,
            unit: "sat".to_string(),
            sender: None,
            timestamp: None,
            state: "settled".to_string(),
        }]);
        let bytes = encode_wallet_projection(&projection);
        let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
        assert_eq!(decoded.recent_history[0].kind, variant);
    }
}

#[test]
fn each_operation_kind_and_state_round_trips() {
    let kinds = [
        WalletOperationKind::SelectBackend,
        WalletOperationKind::PayBolt11,
        WalletOperationKind::CreateCashuWallet,
        WalletOperationKind::PublishNutzapInfo,
        WalletOperationKind::SendNutzap,
        WalletOperationKind::RedeemNutzap,
        WalletOperationKind::DepositCashu,
        WalletOperationKind::MeltCashu,
    ];
    let states = [
        WalletOperationState::Draft,
        WalletOperationState::Prepared,
        WalletOperationState::MintPending,
        WalletOperationState::MintSettled,
        WalletOperationState::PublishPending,
        WalletOperationState::Settled,
        WalletOperationState::Unknown,
        WalletOperationState::Failed,
    ];
    for (kind, state) in kinds.into_iter().zip(states) {
        let projection = WalletProjection::empty().with_pending_operations([WalletOperation::new(
            WalletOperationId::new("op"),
            kind,
            state,
        )]);
        let bytes = encode_wallet_projection(&projection);
        let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
        assert_eq!(decoded.pending_operations[0].kind, kind);
        assert_eq!(decoded.pending_operations[0].state, state);
    }
}

#[test]
fn encoded_buffer_carries_the_nwmp_file_identifier() {
    let bytes = encode_wallet_projection(&fully_populated());
    assert!(super::generated::nmp::wallet::wallet_projection_buffer_has_identifier(&bytes));
    assert_eq!(FILE_IDENTIFIER, b"NWMP");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    assert!(decode_wallet_projection(&[]).is_err());
    assert!(decode_wallet_projection(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_constants_are_stable() {
    assert_eq!(SCHEMA_ID, "nmp.wallet.merged");
    assert_eq!(PROJECTION_KEY, "wallet.merged");
    assert_eq!(SCHEMA_VERSION, 2);
}
