//! Row/table encode + decode helpers for the merged wallet projection wire
//! codec: balances, pending operations (+ consumed inputs), recent history,
//! receive rows, and the capabilities table.
//!
//! Extracted from `projection_wire.rs` to keep each file under the 500-LOC
//! hard cap.

use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, Vector, WIPOffset};

use super::enums::{
    history_kind_from_fb, history_kind_to_fb, operation_kind_from_fb, operation_kind_to_fb,
    operation_state_from_fb, operation_state_to_fb,
};
use super::generated::nmp::wallet as fb;

use crate::capability::WalletCapabilities;
use crate::journal::{WalletConsumedInput, WalletOperation, WalletOperationId};
use crate::projection::{WalletBalanceRow, WalletHistoryRow, WalletReceiveRow};

// --- encode ---------------------------------------------------------------

pub(super) fn encode_capabilities<'a>(
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

pub(super) fn encode_balances<'a>(
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

pub(super) fn encode_operations<'a>(
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
        let recorded_sender = op
            .recorded_sender
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
                has_recorded_amount: op.recorded_amount.is_some(),
                recorded_amount: op.recorded_amount.unwrap_or(0),
                has_recorded_sender: op.recorded_sender.is_some(),
                recorded_sender,
                has_recorded_at: op.recorded_at.is_some(),
                recorded_at: op.recorded_at.unwrap_or(0),
            },
        ));
    }
    fbb.create_vector(&offsets)
}

pub(super) fn encode_history<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[WalletHistoryRow],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletHistoryRow<'a>>>> {
    let mut offsets = Vec::with_capacity(rows.len());
    for row in rows {
        let operation_id = fbb.create_string(&row.operation_id);
        let unit = fbb.create_string(&row.unit);
        let state = fbb.create_string(&row.state);
        let sender = row.sender.as_ref().map(|value| fbb.create_string(value));
        offsets.push(fb::WalletHistoryRow::create(
            fbb,
            &fb::WalletHistoryRowArgs {
                operation_id: Some(operation_id),
                kind: history_kind_to_fb(row.kind),
                amount: row.amount,
                unit: Some(unit),
                has_sender: row.sender.is_some(),
                sender,
                has_timestamp: row.timestamp.is_some(),
                timestamp: row.timestamp.unwrap_or(0),
                state: Some(state),
            },
        ));
    }
    fbb.create_vector(&offsets)
}

pub(super) fn encode_receive_rows<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[WalletReceiveRow],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::WalletReceiveRow<'a>>>> {
    let mut offsets = Vec::with_capacity(rows.len());
    for row in rows {
        let event_id = fbb.create_string(&row.event_id);
        let mint = fbb.create_string(&row.mint);
        let unit = fbb.create_string(&row.unit);
        let sender = row.sender.as_ref().map(|value| fbb.create_string(value));
        offsets.push(fb::WalletReceiveRow::create(
            fbb,
            &fb::WalletReceiveRowArgs {
                event_id: Some(event_id),
                mint: Some(mint),
                amount: row.amount,
                unit: Some(unit),
                has_sender: row.sender.is_some(),
                sender,
                has_timestamp: row.timestamp.is_some(),
                timestamp: row.timestamp.unwrap_or(0),
                accepted: row.accepted,
            },
        ));
    }
    fbb.create_vector(&offsets)
}

// --- decode ---------------------------------------------------------------

pub(super) fn decode_capabilities(
    capabilities: Option<fb::WalletCapabilities<'_>>,
) -> WalletCapabilities {
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

pub(super) fn decode_balances(
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

pub(super) fn decode_operations(
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
        let recorded_amount = row.has_recorded_amount().then(|| row.recorded_amount());
        let recorded_sender = if row.has_recorded_sender() {
            Some(str_field(
                row.recorded_sender(),
                "WalletOperation.recorded_sender",
            )?)
        } else {
            None
        };
        let recorded_at = row.has_recorded_at().then(|| row.recorded_at());
        out.push(WalletOperation {
            id: WalletOperationId::new(str_field(row.id(), "WalletOperation.id")?),
            kind: operation_kind_from_fb(row.kind())?,
            state: operation_state_from_fb(row.state())?,
            correlation_id,
            consumed_inputs: decode_consumed_inputs(row.consumed_inputs())?,
            recorded_amount,
            recorded_sender,
            recorded_at,
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

pub(super) fn decode_history(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletHistoryRow<'_>>>>,
) -> Result<Vec<WalletHistoryRow>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let sender = if row.has_sender() {
            Some(str_field(row.sender(), "WalletHistoryRow.sender")?)
        } else {
            None
        };
        out.push(WalletHistoryRow {
            operation_id: str_field(row.operation_id(), "WalletHistoryRow.operation_id")?,
            kind: history_kind_from_fb(row.kind())?,
            amount: row.amount(),
            unit: str_field(row.unit(), "WalletHistoryRow.unit")?,
            sender,
            timestamp: row.has_timestamp().then(|| row.timestamp()),
            state: str_field(row.state(), "WalletHistoryRow.state")?,
        });
    }
    Ok(out)
}

pub(super) fn decode_receive_rows(
    rows: Option<Vector<'_, ForwardsUOffset<fb::WalletReceiveRow<'_>>>>,
) -> Result<Vec<WalletReceiveRow>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let sender = if row.has_sender() {
            Some(str_field(row.sender(), "WalletReceiveRow.sender")?)
        } else {
            None
        };
        out.push(WalletReceiveRow {
            event_id: str_field(row.event_id(), "WalletReceiveRow.event_id")?,
            mint: str_field(row.mint(), "WalletReceiveRow.mint")?,
            amount: row.amount(),
            unit: str_field(row.unit(), "WalletReceiveRow.unit")?,
            sender,
            timestamp: row.has_timestamp().then(|| row.timestamp()),
            accepted: row.accepted(),
        });
    }
    Ok(out)
}

/// Require a present, non-absent string field; an absent FlatBuffers string on
/// a mandatory slot is a decode error.
pub(super) fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}
