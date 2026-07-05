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
    WalletBalanceRow, WalletDiscoveredMint, WalletHistoryKind, WalletHistoryRow, WalletProjection,
    WalletReadiness, WalletReceiveRow,
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
        // #3008 — not yet carried on the `WalletOperation` wire table (see
        // `rows.rs::decode_operations`'s doc comment); a fully-populated
        // fixture used for wire round-tripping must match what the codec
        // actually preserves, so these stay `None` here even though a real
        // in-flight cross-mint-funded send could have them set in memory.
        recorded_fee_sats: None,
        recorded_cross_mint_source: None,
        recorded_cross_mint_fee_sats: None,
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
            source_mint: Some("https://source-mint.example".to_string()),
            target_mint: Some("https://mint.example".to_string()),
            fee_paid_sats: Some(3),
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
        discovered_mints: vec![
            WalletDiscoveredMint {
                url: "https://mint.example".to_string(),
                name: Some("Example Mint".to_string()),
                nuts: vec![1, 2, 4, 7, 11, 12],
                units: vec!["sat".to_string()],
                supports_nutzap: true,
                trust_score: 5,
                recommendation_count: 2,
            },
            WalletDiscoveredMint {
                url: "https://mint.two".to_string(),
                name: None,
                nuts: vec![],
                units: vec![],
                supports_nutzap: false,
                trust_score: 0,
                recommendation_count: 0,
            },
        ],
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
    assert!(decoded.discovered_mints.is_empty());
}

/// #2880 (epic #2864) — `WalletDiscoveredMint` round-trips both a fully
/// populated row (`name` present, NUTs/units carried) and a bare row (`name`
/// absent, empty NUTs/units) — the same "populated vs. bare" precedent
/// `round_trips_operation_with_no_correlation_id_or_inputs` establishes for
/// `WalletOperation`.
#[test]
fn discovered_mint_rows_round_trip_with_and_without_a_name() {
    let projection = fully_populated();
    let bytes = encode_wallet_projection(&projection);
    let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded.discovered_mints, projection.discovered_mints);

    let with_name = &decoded.discovered_mints[0];
    assert_eq!(with_name.name.as_deref(), Some("Example Mint"));
    assert_eq!(with_name.nuts, vec![1, 2, 4, 7, 11, 12]);
    assert_eq!(with_name.units, vec!["sat".to_string()]);
    assert!(with_name.supports_nutzap);
    assert_eq!(with_name.trust_score, 5);
    assert_eq!(with_name.recommendation_count, 2);

    let without_name = &decoded.discovered_mints[1];
    assert!(without_name.name.is_none());
    assert!(without_name.nuts.is_empty());
    assert!(without_name.units.is_empty());
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
    assert!(op.recorded_fee_sats.is_none());
    assert!(op.recorded_cross_mint_source.is_none());
    assert!(op.recorded_cross_mint_fee_sats.is_none());
}

/// #3008 — `WalletHistoryRow`'s new `source_mint`/`target_mint`/
/// `fee_paid_sats` fields round-trip both `Some` (the cross-mint case) and
/// `None` (an intra-mint send/deposit/redeem that never sets them).
#[test]
fn history_row_source_target_fee_fields_round_trip() {
    let with_fields = WalletHistoryRow {
        operation_id: "op-cross".to_string(),
        kind: WalletHistoryKind::SendNutzap,
        amount: 100,
        unit: "sat".to_string(),
        sender: None,
        timestamp: Some(1_700_000_300),
        state: "settled".to_string(),
        source_mint: Some("https://source-mint.example".to_string()),
        target_mint: Some("https://target-mint.example".to_string()),
        fee_paid_sats: Some(6),
    };
    let without_fields = WalletHistoryRow {
        operation_id: "op-intra".to_string(),
        kind: WalletHistoryKind::Deposit,
        amount: 50,
        unit: "sat".to_string(),
        sender: None,
        timestamp: None,
        state: "settled".to_string(),
        source_mint: None,
        target_mint: None,
        fee_paid_sats: None,
    };
    let projection = WalletProjection::empty()
        .with_recent_history([with_fields.clone(), without_fields.clone()]);
    let bytes = encode_wallet_projection(&projection);
    let decoded = decode_wallet_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded.recent_history[0], with_fields);
    assert_eq!(decoded.recent_history[1], without_fields);
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
            source_mint: None,
            target_mint: None,
            fee_paid_sats: None,
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
        WalletOperationKind::SetCashuMints,
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
        // #2997 — the 9th kind (`SetCashuMints`) reuses `Draft`; every state
        // is already covered by an earlier pairing above, so this only needs
        // to prove `SetCashuMints` itself round-trips.
        WalletOperationState::Draft,
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
    assert_eq!(SCHEMA_VERSION, 3);
}
