//! `CashuWalletBackend::snapshot()`'s `receive_rows`/`recent_history`
//! derivation — split out of `mod.rs` (AGENTS.md file-size discipline), #2949.
//!
//! Both fields fold `state.journal.terminal_operations()` — the operations
//! `pending_operations()` stops showing the instant they settle or fail (see
//! `journal::saga`'s doc comment on that method). `DepositCashu` never
//! records `consumed_inputs` (depositing mints fresh proofs rather than
//! consuming any — see `saga::WalletOperationKind::
//! requires_consumed_inputs_before_mint_request`), so its amount/mint comes
//! from `state.pending_deposits` instead, matched by operation id.
//!
//! Never proofs, secrets, or quote ids (design doc "Privacy And Security") —
//! only the public fields `WalletConsumedInput`/`PendingDeposit` already
//! carry.
//!
//! # Why a terminal `RedeemNutzap` operation always has a receive row
//!
//! `redeem.rs`'s `fail` records a best-effort consumed input (event id
//! always, mint/amount once the nutzap decodes) before transitioning to
//! `Failed`, so a deliberately-unverifiable nutzap surfaces here as a
//! rejected (`accepted: false`) candidate rather than vanishing — see the
//! design doc's "Observer counting: unverifiable nutzaps may be shown as
//! rejected... not counted as value".

use crate::journal::{WalletOperation, WalletOperationKind, WalletOperationState};
use crate::projection::{WalletHistoryKind, WalletHistoryRow, WalletReceiveRow};

use super::state::CashuWalletState;

/// `receive_rows`: every terminal `RedeemNutzap` operation, verified
/// (`accepted: true`, reached `Settled`) or rejected (`accepted: false`,
/// reached `Failed`) alike — never silently absent. Takes the same
/// `terminal` slice `recent_history` does (see `mod.rs`'s `snapshot()`) so
/// `state.journal.terminal_operations()` is computed once per snapshot, not
/// once per field.
pub(super) fn receive_rows(terminal: &[WalletOperation]) -> Vec<WalletReceiveRow> {
    terminal
        .iter()
        .filter(|op| op.kind == WalletOperationKind::RedeemNutzap)
        .map(|op| {
            let input = op.consumed_inputs.last();
            WalletReceiveRow {
                event_id: input.map_or_else(String::new, |i| i.event_id.clone()),
                mint: input.map_or_else(String::new, |i| i.mint.clone()),
                amount: input.map_or(0, |i| i.amount),
                unit: input.map_or_else(|| "sat".to_string(), |i| i.unit.clone()),
                accepted: op.state == WalletOperationState::Settled,
            }
        })
        .collect()
}

/// `recent_history`: every terminal operation whose kind has a
/// `WalletHistoryKind` counterpart (deposits, sends, redeems — the
/// balance-changing intents; `CreateCashuWallet`/`PublishNutzapInfo` are
/// setup operations with no history-row shape, so `history_kind` skips
/// them).
pub(super) fn recent_history(
    state: &CashuWalletState,
    terminal: &[WalletOperation],
) -> Vec<WalletHistoryRow> {
    terminal
        .iter()
        .filter_map(|op| history_row(state, op))
        .collect()
}

fn history_row(state: &CashuWalletState, op: &WalletOperation) -> Option<WalletHistoryRow> {
    let kind = history_kind(op.kind)?;
    let (amount, unit) = match op.consumed_inputs.last() {
        Some(input) => (input.amount, input.unit.clone()),
        None => deposit_amount(state, op).unwrap_or((0, "sat".to_string())),
    };
    Some(WalletHistoryRow {
        operation_id: op.id.as_str().to_string(),
        kind,
        amount,
        unit,
        state: format!("{:?}", op.state),
    })
}

fn history_kind(op_kind: WalletOperationKind) -> Option<WalletHistoryKind> {
    match op_kind {
        WalletOperationKind::DepositCashu => Some(WalletHistoryKind::Deposit),
        WalletOperationKind::SendNutzap => Some(WalletHistoryKind::SendNutzap),
        WalletOperationKind::RedeemNutzap => Some(WalletHistoryKind::RedeemNutzap),
        WalletOperationKind::PayBolt11
        | WalletOperationKind::CreateCashuWallet
        | WalletOperationKind::PublishNutzapInfo
        | WalletOperationKind::SelectBackend
        | WalletOperationKind::MeltCashu => None,
    }
}

/// `DepositCashu`'s amount/mint, recovered from `PendingDeposit` (never
/// cleared once set — see `state.rs`'s doc comment on that field) since the
/// operation's own `consumed_inputs` stays empty for this kind.
fn deposit_amount(state: &CashuWalletState, op: &WalletOperation) -> Option<(u64, String)> {
    state
        .pending_deposits
        .values()
        .find(|pending| pending.operation_id == op.id)
        .map(|pending| (pending.amount_sats, "sat".to_string()))
}
