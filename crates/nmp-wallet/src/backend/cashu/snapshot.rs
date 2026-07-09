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

use std::collections::BTreeSet;

use nmp_nip60::cashu::canonicalize_mint_url;

use crate::journal::{WalletOperation, WalletOperationKind, WalletOperationState};
use crate::projection::{
    WalletBalanceRow, WalletHistoryKind, WalletHistoryRow, WalletMintInfoRow, WalletReceiveRow,
};

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
            let (event_id, mint, amount, unit) = match op.consumed_inputs.last() {
                Some(input) => (
                    input.event_id.clone(),
                    input.mint.clone(),
                    input.amount,
                    input.unit.clone(),
                ),
                None => (String::new(), String::new(), 0, "sat".to_string()),
            };
            WalletReceiveRow {
                event_id,
                mint,
                amount,
                unit,
                // #2966 — set by `RedeemNutzapCommand::run` (`redeem.rs`)
                // the moment the kind:9321 event resolves, and by
                // `begin_operation_at` at dispatch time respectively; see
                // `WalletOperation::recorded_sender`/`recorded_at`'s doc
                // comments.
                sender: op.recorded_sender.clone(),
                timestamp: op.recorded_at,
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
    // #2966 — `SendNutzap` is special-cased to `recorded_amount` (the
    // amount the sender intended to deliver, recorded up front by
    // `nutzap_dispatch.rs`'s `start_send_nutzap`): `consumed_inputs` names
    // whichever wallet-internal proof denominations got selected to cover
    // it, which is a different number (and empty entirely for a send that
    // fails before proof selection) — see
    // `WalletOperation::recorded_amount`'s doc comment.
    let (amount, unit) = match (op.kind, op.recorded_amount) {
        (WalletOperationKind::SendNutzap, Some(amount)) => (amount, "sat".to_string()),
        _ => match op.consumed_inputs.last() {
            Some(input) => (input.amount, input.unit.clone()),
            None => deposit_amount(state, op).unwrap_or((0, "sat".to_string())),
        },
    };
    // #3008 — which mint(s) a payment used and its fee, without decoding a
    // proof. Only `SendNutzap` has a meaningful source/target-mint split
    // (a deposit/redeem has no "sent from a mint" concept); `target_mint`
    // is whichever mint `consumed_inputs` actually names (the mint the
    // P2PK swap ran at — the SAME mint the recipient's kind:9321 `u` tag
    // carries), `source_mint` defaults to that same mint (the ordinary,
    // intra-mint case) unless the cross-mint auto-fallback recorded a
    // DIFFERENT melt-source mint on this operation (see
    // `cross_mint_publish.rs::dispatch_cross_mint_token_event`).
    // `fee_paid_sats` folds this send's own swap fee with any recorded
    // melt fee from that fallback; `None` only if the send never reached
    // the swap (so no fee was ever recorded) at all.
    let (source_mint, target_mint, fee_paid_sats) = if op.kind == WalletOperationKind::SendNutzap {
        let target_mint = op.consumed_inputs.last().map(|input| input.mint.clone());
        let source_mint = op
            .recorded_cross_mint_source
            .clone()
            .or_else(|| target_mint.clone());
        let fee_paid_sats = op
            .recorded_fee_sats
            .map(|own_fee| own_fee + op.recorded_cross_mint_fee_sats.unwrap_or(0));
        (source_mint, target_mint, fee_paid_sats)
    } else {
        (None, None, None)
    };
    Some(WalletHistoryRow {
        operation_id: op.id.as_str().to_string(),
        kind,
        amount,
        unit,
        sender: op.recorded_sender.clone(),
        timestamp: op.recorded_at,
        state: format!("{:?}", op.state),
        source_mint,
        target_mint,
        fee_paid_sats,
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
        | WalletOperationKind::MeltCashu
        | WalletOperationKind::SetCashuMints
        // #3003 — no dedicated `WalletHistoryKind` row for cross-mint
        // transfers yet (they surface as the recipient's eventual
        // `SendNutzap` history row instead); a future UI-facing history kind
        // is a fast-follow, not required for the money-safety saga itself.
        | WalletOperationKind::CrossMintTransfer => None,
    }
}

/// `mint_info`: raw NUT-06/NUT-02 metadata for every WALLET-RELEVANT mint
/// (#3030, PR2 of 2) — accepted mints, mints this wallet holds a balance at,
/// and mints named by a recent send/receive row — read ONLY from
/// `state.mint_info`'s cache (never fetched here; see `mint_info.rs`'s module
/// docs for the off-projection-path fetch that populates it). A relevant mint
/// with no cache entry yet (never successfully fetched) simply contributes no
/// row — never an error. Deterministic order: `BTreeSet` (below) sorts by
/// canonical URL, so identical wallet state always encodes to identical bytes
/// (the byte-equality emission compare in `projections-and-emission.md`).
pub(super) fn mint_info_rows(
    state: &CashuWalletState,
    balances: &[WalletBalanceRow],
    history: &[WalletHistoryRow],
    receive_rows: &[WalletReceiveRow],
) -> Vec<WalletMintInfoRow> {
    wallet_relevant_mint_urls(state, balances, history, receive_rows)
        .into_iter()
        .filter_map(|url| {
            state
                .mint_info
                .get(&url)
                .map(|cached| cached.to_row(url))
        })
        .collect()
}

/// Every canonical mint URL this wallet currently cares about: the accepted
/// list (`state.mints`), every balance row's mint, and every recent-history/
/// receive-row mint field (source/target mint for a send, the mint for a
/// deposit/redeem) — canonicalized so this set matches
/// `state.mint_info`'s canonical-URL cache keys regardless of whichever raw
/// spelling `state.mints` (or a caller-supplied mint string upstream) used.
fn wallet_relevant_mint_urls(
    state: &CashuWalletState,
    balances: &[WalletBalanceRow],
    history: &[WalletHistoryRow],
    receive_rows: &[WalletReceiveRow],
) -> BTreeSet<String> {
    let mut urls = BTreeSet::new();
    for mint in &state.mints {
        urls.insert(canonicalize_mint_url(mint));
    }
    for balance in balances {
        urls.insert(canonicalize_mint_url(&balance.mint));
    }
    for row in history {
        if let Some(mint) = &row.source_mint {
            urls.insert(canonicalize_mint_url(mint));
        }
        if let Some(mint) = &row.target_mint {
            urls.insert(canonicalize_mint_url(mint));
        }
    }
    for row in receive_rows {
        urls.insert(canonicalize_mint_url(&row.mint));
    }
    urls
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
