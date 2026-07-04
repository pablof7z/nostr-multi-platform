//! Typed FlatBuffers wire codec for the MERGED multi-backend
//! [`crate::projection::WalletProjection`] (#2915, epic #2864).
//!
//! This is the typed sidecar counterpart to the serde JSON of `WalletProjection`.
//! `nmp_wallet::register` registers it under the DISTINCT projection key
//! `"wallet.merged"` — deliberately NOT `"wallet"`, which `nmp-nip47` still owns
//! for its single-backend NWC `WalletStatus` shape
//! (`crates/nmp-nip47/src/wire/typed_fb.rs`, `NWST`). The two coexist as separate
//! typed sidecars: an NWC-only host keeps decoding the `NWST` `"wallet"` payload;
//! a host that wants the merged backend-selection + capability-union +
//! concatenated bounded rows decodes this `NWMP` `"wallet.merged"` payload. Both
//! are emitted ALONGSIDE the generic `Value` projection, never replacing it
//! (ADR-0072).
//!
//! The schema (`crates/nmp-wallet/schema/wallet_projection.fbs`) mirrors the Rust
//! structs field-for-field. `Option<...>` fields carry a `has_*` presence flag
//! plus the value so absent (`None`) round-trips distinctly from a present
//! default — the same optional-fields convention `wallet_status.fbs` uses. The
//! nested `balances`/`pending_operations`/`recent_history`/`receive_rows` vectors
//! follow the `NotificationsSnapshot`/`ModularTimelineSnapshot` vector-of-tables
//! precedent.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This `allow` block scopes the relaxation to the
// single generated module — no hand-written code in this file uses `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/wallet_projection_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, Vector, WIPOffset};

use generated::nmp::wallet as fb;

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

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.wallet.merged";
/// The projection key this typed sidecar registers under. Distinct from
/// `nmp-nip47`'s `"wallet"` key (see module docs).
pub const PROJECTION_KEY: &str = "wallet.merged";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NWMP";
/// Wire schema version. Bump on any breaking change to `wallet_projection.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- enum bridges ---------------------------------------------------------

fn readiness_to_fb(readiness: WalletReadiness) -> fb::WalletReadiness {
    match readiness {
        WalletReadiness::NotConfigured => fb::WalletReadiness::NotConfigured,
        WalletReadiness::Activating => fb::WalletReadiness::Activating,
        WalletReadiness::Ready => fb::WalletReadiness::Ready,
        WalletReadiness::Degraded => fb::WalletReadiness::Degraded,
    }
}

fn readiness_from_fb(readiness: fb::WalletReadiness) -> Result<WalletReadiness, String> {
    match readiness {
        fb::WalletReadiness::NotConfigured => Ok(WalletReadiness::NotConfigured),
        fb::WalletReadiness::Activating => Ok(WalletReadiness::Activating),
        fb::WalletReadiness::Ready => Ok(WalletReadiness::Ready),
        fb::WalletReadiness::Degraded => Ok(WalletReadiness::Degraded),
        other => Err(format!("unknown WalletReadiness discriminant {}", other.0)),
    }
}

fn history_kind_to_fb(kind: WalletHistoryKind) -> fb::WalletHistoryKind {
    match kind {
        WalletHistoryKind::Deposit => fb::WalletHistoryKind::Deposit,
        WalletHistoryKind::SendNutzap => fb::WalletHistoryKind::SendNutzap,
        WalletHistoryKind::RedeemNutzap => fb::WalletHistoryKind::RedeemNutzap,
        WalletHistoryKind::PayBolt11 => fb::WalletHistoryKind::PayBolt11,
    }
}

fn history_kind_from_fb(kind: fb::WalletHistoryKind) -> Result<WalletHistoryKind, String> {
    match kind {
        fb::WalletHistoryKind::Deposit => Ok(WalletHistoryKind::Deposit),
        fb::WalletHistoryKind::SendNutzap => Ok(WalletHistoryKind::SendNutzap),
        fb::WalletHistoryKind::RedeemNutzap => Ok(WalletHistoryKind::RedeemNutzap),
        fb::WalletHistoryKind::PayBolt11 => Ok(WalletHistoryKind::PayBolt11),
        other => Err(format!("unknown WalletHistoryKind discriminant {}", other.0)),
    }
}

fn operation_kind_to_fb(kind: WalletOperationKind) -> fb::WalletOperationKind {
    match kind {
        WalletOperationKind::SelectBackend => fb::WalletOperationKind::SelectBackend,
        WalletOperationKind::PayBolt11 => fb::WalletOperationKind::PayBolt11,
        WalletOperationKind::CreateCashuWallet => fb::WalletOperationKind::CreateCashuWallet,
        WalletOperationKind::PublishNutzapInfo => fb::WalletOperationKind::PublishNutzapInfo,
        WalletOperationKind::SendNutzap => fb::WalletOperationKind::SendNutzap,
        WalletOperationKind::RedeemNutzap => fb::WalletOperationKind::RedeemNutzap,
        WalletOperationKind::DepositCashu => fb::WalletOperationKind::DepositCashu,
        WalletOperationKind::MeltCashu => fb::WalletOperationKind::MeltCashu,
    }
}

fn operation_kind_from_fb(kind: fb::WalletOperationKind) -> Result<WalletOperationKind, String> {
    match kind {
        fb::WalletOperationKind::SelectBackend => Ok(WalletOperationKind::SelectBackend),
        fb::WalletOperationKind::PayBolt11 => Ok(WalletOperationKind::PayBolt11),
        fb::WalletOperationKind::CreateCashuWallet => Ok(WalletOperationKind::CreateCashuWallet),
        fb::WalletOperationKind::PublishNutzapInfo => Ok(WalletOperationKind::PublishNutzapInfo),
        fb::WalletOperationKind::SendNutzap => Ok(WalletOperationKind::SendNutzap),
        fb::WalletOperationKind::RedeemNutzap => Ok(WalletOperationKind::RedeemNutzap),
        fb::WalletOperationKind::DepositCashu => Ok(WalletOperationKind::DepositCashu),
        fb::WalletOperationKind::MeltCashu => Ok(WalletOperationKind::MeltCashu),
        other => Err(format!(
            "unknown WalletOperationKind discriminant {}",
            other.0
        )),
    }
}

fn operation_state_to_fb(state: WalletOperationState) -> fb::WalletOperationState {
    match state {
        WalletOperationState::Draft => fb::WalletOperationState::Draft,
        WalletOperationState::Prepared => fb::WalletOperationState::Prepared,
        WalletOperationState::MintPending => fb::WalletOperationState::MintPending,
        WalletOperationState::MintSettled => fb::WalletOperationState::MintSettled,
        WalletOperationState::PublishPending => fb::WalletOperationState::PublishPending,
        WalletOperationState::Settled => fb::WalletOperationState::Settled,
        WalletOperationState::Unknown => fb::WalletOperationState::Unknown,
        WalletOperationState::Failed => fb::WalletOperationState::Failed,
    }
}

fn operation_state_from_fb(state: fb::WalletOperationState) -> Result<WalletOperationState, String> {
    match state {
        fb::WalletOperationState::Draft => Ok(WalletOperationState::Draft),
        fb::WalletOperationState::Prepared => Ok(WalletOperationState::Prepared),
        fb::WalletOperationState::MintPending => Ok(WalletOperationState::MintPending),
        fb::WalletOperationState::MintSettled => Ok(WalletOperationState::MintSettled),
        fb::WalletOperationState::PublishPending => Ok(WalletOperationState::PublishPending),
        fb::WalletOperationState::Settled => Ok(WalletOperationState::Settled),
        fb::WalletOperationState::Unknown => Ok(WalletOperationState::Unknown),
        fb::WalletOperationState::Failed => Ok(WalletOperationState::Failed),
        other => Err(format!(
            "unknown WalletOperationState discriminant {}",
            other.0
        )),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`WalletProjection`] to typed FlatBuffers bytes (with the `NWMP`
/// file identifier).
#[must_use]
pub fn encode_wallet_projection(projection: &WalletProjection) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    // All child offsets (strings, nested tables, vectors) must be created before
    // the root table is started.
    let active_backend_id = projection
        .active_backend_id
        .as_ref()
        .map(|id| fbb.create_string(id.as_str()));
    let cashu_p2pk_pubkey = projection
        .cashu_p2pk_pubkey
        .as_ref()
        .map(|value| fbb.create_string(value));

    let capabilities = encode_capabilities(&mut fbb, &projection.capabilities);
    let balances = encode_balances(&mut fbb, &projection.balances);
    let pending_operations = encode_operations(&mut fbb, &projection.pending_operations);
    let recent_history = encode_history(&mut fbb, &projection.recent_history);
    let receive_rows = encode_receive_rows(&mut fbb, &projection.receive_rows);

    let root = fb::WalletProjection::create(
        &mut fbb,
        &fb::WalletProjectionArgs {
            schema_version: SCHEMA_VERSION,
            has_active_backend_id: projection.active_backend_id.is_some(),
            active_backend_id,
            readiness: readiness_to_fb(projection.readiness),
            capabilities: Some(capabilities),
            balances: Some(balances),
            has_cashu_p2pk_pubkey: projection.cashu_p2pk_pubkey.is_some(),
            cashu_p2pk_pubkey,
            accepted_mint_count: projection.accepted_mint_count,
            accepted_relay_count: projection.accepted_relay_count,
            pending_operations: Some(pending_operations),
            recent_history: Some(recent_history),
            receive_rows: Some(receive_rows),
        },
    );
    fb::finish_wallet_projection_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_capabilities<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    capabilities: &WalletCapabilities,
) -> WIPOffset<fb::WalletCapabilities<'a>> {
    fb::WalletCapabilities::create(
        fbb,
        &fb::WalletCapabilitiesArgs {
            pay_bolt11: capabilities.pay_bolt11,
            create_cashu_wallet: capabilities.create_cashu_wallet,
            publish_nutzap_info: capabilities.publish_nutzap_info,
            send_nutzap: capabilities.send_nutzap,
            redeem_nutzap: capabilities.redeem_nutzap,
            deposit_cashu: capabilities.deposit_cashu,
            melt_cashu: capabilities.melt_cashu,
            observe_nutzap_receipts: capabilities.observe_nutzap_receipts,
        },
    )
}

fn encode_balances<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[WalletBalanceRow],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletBalanceRow<'a>>>> {
    let mut offsets = Vec::with_capacity(rows.len());
    for row in rows {
        let mint = fbb.create_string(&row.mint);
        let unit = fbb.create_string(&row.unit);
        offsets.push(fb::WalletBalanceRow::create(
            fbb,
            &fb::WalletBalanceRowArgs {
                mint: Some(mint),
                unit: Some(unit),
                amount: row.amount,
            },
        ));
    }
    fbb.create_vector(&offsets)
}

fn encode_operations<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    operations: &[WalletOperation],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletOperation<'a>>>> {
    let mut offsets = Vec::with_capacity(operations.len());
    for op in operations {
        // Build the nested consumed-inputs vector first, then the operation table.
        let mut input_offsets = Vec::with_capacity(op.consumed_inputs.len());
        for input in &op.consumed_inputs {
            let event_id = fbb.create_string(&input.event_id);
            let mint = fbb.create_string(&input.mint);
            let unit = fbb.create_string(&input.unit);
            input_offsets.push(fb::WalletConsumedInput::create(
                fbb,
                &fb::WalletConsumedInputArgs {
                    event_id: Some(event_id),
                    mint: Some(mint),
                    unit: Some(unit),
                    amount: input.amount,
                },
            ));
        }
        let consumed_inputs = fbb.create_vector(&input_offsets);

        let id = fbb.create_string(op.id.as_str());
        let correlation_id = op
            .correlation_id
            .as_ref()
            .map(|value| fbb.create_string(value));

        offsets.push(fb::WalletOperation::create(
            fbb,
            &fb::WalletOperationArgs {
                id: Some(id),
                kind: operation_kind_to_fb(op.kind),
                state: operation_state_to_fb(op.state),
                has_correlation_id: op.correlation_id.is_some(),
                correlation_id,
                consumed_inputs: Some(consumed_inputs),
            },
        ));
    }
    fbb.create_vector(&offsets)
}

fn encode_history<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[WalletHistoryRow],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletHistoryRow<'a>>>> {
    let mut offsets = Vec::with_capacity(rows.len());
    for row in rows {
        let operation_id = fbb.create_string(&row.operation_id);
        let unit = fbb.create_string(&row.unit);
        let state = fbb.create_string(&row.state);
        offsets.push(fb::WalletHistoryRow::create(
            fbb,
            &fb::WalletHistoryRowArgs {
                operation_id: Some(operation_id),
                kind: history_kind_to_fb(row.kind),
                amount: row.amount,
                unit: Some(unit),
                state: Some(state),
            },
        ));
    }
    fbb.create_vector(&offsets)
}

fn encode_receive_rows<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[WalletReceiveRow],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletReceiveRow<'a>>>> {
    let mut offsets = Vec::with_capacity(rows.len());
    for row in rows {
        let event_id = fbb.create_string(&row.event_id);
        let mint = fbb.create_string(&row.mint);
        let unit = fbb.create_string(&row.unit);
        offsets.push(fb::WalletReceiveRow::create(
            fbb,
            &fb::WalletReceiveRowArgs {
                event_id: Some(event_id),
                mint: Some(mint),
                amount: row.amount,
                unit: Some(unit),
                accepted: row.accepted,
            },
        ));
    }
    fbb.create_vector(&offsets)
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_wallet_projection`])
/// back into a [`WalletProjection`]. Returns an error string on any malformed
/// input.
pub fn decode_wallet_projection(bytes: &[u8]) -> Result<WalletProjection, String> {
    if bytes.len() < 8 || !fb::wallet_projection_buffer_has_identifier(bytes) {
        return Err("missing NWMP file identifier".to_string());
    }
    let root = fb::root_as_wallet_projection(bytes)
        .map_err(|e| format!("not a valid WalletProjection buffer: {e}"))?;

    let active_backend_id = if root.has_active_backend_id() {
        Some(WalletBackendId::new(str_field(
            root.active_backend_id(),
            "WalletProjection.active_backend_id",
        )?))
    } else {
        None
    };

    let cashu_p2pk_pubkey = if root.has_cashu_p2pk_pubkey() {
        Some(str_field(
            root.cashu_p2pk_pubkey(),
            "WalletProjection.cashu_p2pk_pubkey",
        )?)
    } else {
        None
    };

    let capabilities = decode_capabilities(root.capabilities());
    let balances = decode_balances(root.balances())?;
    let pending_operations = decode_operations(root.pending_operations())?;
    let recent_history = decode_history(root.recent_history())?;
    let receive_rows = decode_receive_rows(root.receive_rows())?;

    Ok(WalletProjection {
        active_backend_id,
        readiness: readiness_from_fb(root.readiness())?,
        capabilities,
        balances,
        cashu_p2pk_pubkey,
        accepted_mint_count: root.accepted_mint_count(),
        accepted_relay_count: root.accepted_relay_count(),
        pending_operations,
        recent_history,
        receive_rows,
    })
}

fn decode_capabilities(capabilities: Option<fb::WalletCapabilities<'_>>) -> WalletCapabilities {
    // An absent capabilities table decodes as the all-false default — the same
    // meaning `WalletCapabilities::none()` carries.
    let Some(caps) = capabilities else {
        return WalletCapabilities::none();
    };
    WalletCapabilities {
        pay_bolt11: caps.pay_bolt11(),
        create_cashu_wallet: caps.create_cashu_wallet(),
        publish_nutzap_info: caps.publish_nutzap_info(),
        send_nutzap: caps.send_nutzap(),
        redeem_nutzap: caps.redeem_nutzap(),
        deposit_cashu: caps.deposit_cashu(),
        melt_cashu: caps.melt_cashu(),
        observe_nutzap_receipts: caps.observe_nutzap_receipts(),
    }
}

fn decode_balances(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletBalanceRow<'_>>>>,
) -> Result<Vec<WalletBalanceRow>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WalletBalanceRow {
            mint: str_field(row.mint(), "WalletBalanceRow.mint")?,
            unit: str_field(row.unit(), "WalletBalanceRow.unit")?,
            amount: row.amount(),
        });
    }
    Ok(out)
}

fn decode_operations(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletOperation<'_>>>>,
) -> Result<Vec<WalletOperation>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let correlation_id = if row.has_correlation_id() {
            Some(str_field(
                row.correlation_id(),
                "WalletOperation.correlation_id",
            )?)
        } else {
            None
        };
        out.push(WalletOperation {
            id: WalletOperationId::new(str_field(row.id(), "WalletOperation.id")?),
            kind: operation_kind_from_fb(row.kind())?,
            state: operation_state_from_fb(row.state())?,
            correlation_id,
            consumed_inputs: decode_consumed_inputs(row.consumed_inputs())?,
        });
    }
    Ok(out)
}

fn decode_consumed_inputs(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletConsumedInput<'_>>>>,
) -> Result<Vec<WalletConsumedInput>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WalletConsumedInput {
            event_id: str_field(row.event_id(), "WalletConsumedInput.event_id")?,
            mint: str_field(row.mint(), "WalletConsumedInput.mint")?,
            unit: str_field(row.unit(), "WalletConsumedInput.unit")?,
            amount: row.amount(),
        });
    }
    Ok(out)
}

fn decode_history(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletHistoryRow<'_>>>>,
) -> Result<Vec<WalletHistoryRow>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WalletHistoryRow {
            operation_id: str_field(row.operation_id(), "WalletHistoryRow.operation_id")?,
            kind: history_kind_from_fb(row.kind())?,
            amount: row.amount(),
            unit: str_field(row.unit(), "WalletHistoryRow.unit")?,
            state: str_field(row.state(), "WalletHistoryRow.state")?,
        });
    }
    Ok(out)
}

fn decode_receive_rows(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletReceiveRow<'_>>>>,
) -> Result<Vec<WalletReceiveRow>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WalletReceiveRow {
            event_id: str_field(row.event_id(), "WalletReceiveRow.event_id")?,
            mint: str_field(row.mint(), "WalletReceiveRow.mint")?,
            amount: row.amount(),
            unit: str_field(row.unit(), "WalletReceiveRow.unit")?,
            accepted: row.accepted(),
        });
    }
    Ok(out)
}

/// Require a present, non-absent string field; an absent FlatBuffers string on
/// a mandatory slot is a decode error.
fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

#[cfg(test)]
#[path = "projection_wire_tests.rs"]
mod tests;
