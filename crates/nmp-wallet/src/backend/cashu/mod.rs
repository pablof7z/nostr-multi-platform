//! The Cashu [`WalletBackend`] adapter (#2895 W2, epic #2864) — the second
//! concrete backend behind the seam, alongside `backend::nwc`.
//!
//! This adapter owns:
//!
//! * `capabilities()` advertises exactly `create_cashu_wallet` +
//!   `deposit_cashu` — this is the actual implemented scope; nutzap
//!   send/receive/publish-info and mint-probe observation are separate epic
//!   #2864 waves this backend does not implement yet (see
//!   `WalletCapabilities::cashu_wallet_and_deposit`'s doc comment).
//! * `start_intent()` maps `CreateCashuWallet`/`DepositQuote`/`CompleteDeposit`
//!   onto `ActorCommand::Protocol` commands (`create_wallet.rs`, `deposit.rs`)
//!   that drive the signer-transparent NIP-44 + sign ports and `nmp-nip60`'s
//!   mint HTTP lane. Every other intent variant is NWC/nutzap-shaped and this
//!   backend does not implement it, so `start_intent` is a documented no-op
//!   for those (never a panic — D6), same as `backend::nwc` treats its own
//!   out-of-capability intents.
//! * `snapshot()` reads the shared [`state::CashuWalletState`] this backend's
//!   own commands/workers write (D4: this backend's commands are the sole
//!   writer, mirroring `NwcWalletBackend`'s `WalletStatusSlot`).
//!
//! # NOT wired here (escalated, not silently skipped)
//!
//! * `on_wallet_event` — reconciling kind:17375/7375/7376 durable events into
//!   `WalletLedger::rebuild_from` on cold start is live-wiring work (this
//!   backend's in-memory state today is populated only by its own
//!   `start_intent`-driven operations, not by replaying the kernel's fetched
//!   event stream). A documented no-op, not a silent partial read.
//! * `on_mint_result` — this backend's own `ProtocolCommand` workers
//!   (`CashuDepositQuoteCommand`, `CashuCompleteDepositCommand`) own the full
//!   mint-round-trip -> state/publish mapping directly against the richer
//!   data the mint response actually carries (quote id, bolt11, proofs); the
//!   coarse `MintResult` seam (operation_id + Settled/Failed/Unknown) cannot
//!   carry that shape, and nothing constructs one for this backend today.
//!   Mirrors `NwcWalletBackend::on_mint_result`'s treatment of the same seam
//!   for the opposite reason ("not this backend's concept").
//!
//! Live actor wiring end-to-end (registering this backend, driving
//! `on_wallet_event` from the kernel's fetch pipeline) is epic #2864's W7.

mod chain;
mod create_wallet;
mod deposit;
mod state;
mod ui_codes;

use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActionLedgerCommand, ActorCommand};
use nmp_core::substrate::{KernelEvent, ProtocolCommandContext};
use nmp_core::ui_token::UiToken;

use crate::journal::{WalletOperationId, WalletOperationKind, WalletOperationState};
use crate::projection::{WalletBalanceRow, WalletProjection, WalletReadiness};

use super::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
use crate::capability::WalletCapabilities;

use create_wallet::CreateCashuWalletCommand;
use deposit::{CashuCompleteDepositCommand, CashuDepositQuoteCommand};
use state::{is_well_formed_mint_url, lock_state, CashuWalletState};

/// Canonical id this backend registers under.
pub const CASHU_BACKEND_ID: &str = "cashu";

/// [`WalletBackend`] adapter over `nmp-nip60`'s wallet-config codec + mint
/// HTTP lane.
pub struct CashuWalletBackend {
    state: Arc<Mutex<CashuWalletState>>,
}

impl CashuWalletBackend {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CashuWalletState::new())),
        }
    }
}

impl Default for CashuWalletBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WalletBackend for CashuWalletBackend {
    fn id(&self) -> WalletBackendId {
        WalletBackendId::new(CASHU_BACKEND_ID)
    }

    fn capabilities(&self) -> WalletCapabilities {
        WalletCapabilities::cashu_wallet_and_deposit()
    }

    fn snapshot(&self, _scope: WalletProjectionScope) -> WalletBackendSnapshot {
        let state = lock_state(&self.state);
        let readiness = if state.created {
            WalletReadiness::Ready
        } else {
            WalletReadiness::NotConfigured
        };
        let mut projection = WalletProjection::new(Some(self.id()), readiness, self.capabilities())
            .with_pending_operations(state.journal.pending_operations());
        projection.cashu_p2pk_pubkey = state.cashu_pubkey_hex.clone();
        projection.accepted_mint_count = state.mints.len() as u32;
        let balances: Vec<WalletBalanceRow> = state
            .ledger
            .state()
            .balances()
            .into_iter()
            .map(|(key, amount_msat)| WalletBalanceRow {
                mint: key.mint.as_str().to_string(),
                unit: key.unit.as_str().to_string(),
                amount: amount_msat / 1000,
            })
            .collect();
        projection = projection.with_balances(balances);
        WalletBackendSnapshot { projection }
    }

    fn start_intent(
        &self,
        ctx: WalletBackendContext<'_>,
        intent: WalletIntent,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        match intent {
            WalletIntent::CreateCashuWallet { mint } => {
                self.start_create_wallet(ctx, mint, correlation_id)
            }
            WalletIntent::DepositQuote { mint, amount_sats } => {
                self.start_deposit_quote(ctx, mint, amount_sats, correlation_id)
            }
            WalletIntent::CompleteDeposit { quote_id } => {
                self.start_complete_deposit(ctx, quote_id, correlation_id)
            }
            // Not this backend's capability — `capabilities()` already tells
            // callers not to route these here. A no-op rather than a panic
            // keeps a stray dispatch harmless (D6), same as `backend::nwc`.
            // `RecoverCashuWallet` is nominally reachable via
            // `create_cashu_wallet`'s action-namespace bundling
            // (`WalletCapabilities::action_namespaces`) but is separate
            // epic #2864 scope this backend does not implement yet.
            WalletIntent::SelectBackend { .. }
            | WalletIntent::PayBolt11 { .. }
            | WalletIntent::RecoverCashuWallet
            | WalletIntent::PublishNutzapInfo
            | WalletIntent::SendNutzap { .. }
            | WalletIntent::RedeemNutzap { .. }
            | WalletIntent::MeltCashu { .. } => Vec::new(),
        }
    }

    fn on_wallet_event(
        &self,
        _ctx: WalletBackendContext<'_>,
        _event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        // See the module doc comment: cold-start reconciliation from the
        // kernel's fetched event stream is live-wiring (W7), not wired here.
        Vec::new()
    }

    fn on_mint_result(
        &self,
        _ctx: WalletBackendContext<'_>,
        _result: MintResult,
    ) -> Vec<ActorCommand> {
        // See the module doc comment: this backend's own workers own the
        // mint-result mapping directly against the richer data they hold.
        Vec::new()
    }
}

impl CashuWalletBackend {
    fn start_create_wallet(
        &self,
        ctx: WalletBackendContext<'_>,
        mint: String,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(ui_codes::NO_ACCOUNT, correlation_id, "no active account".to_string());
        };
        if !is_well_formed_mint_url(&mint) {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                format!("unsupported mint: {mint}"),
            );
        }
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "create");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation(operation_id.clone(), WalletOperationKind::CreateCashuWallet) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
        }
        vec![ActorCommand::Protocol(Box::new(CreateCashuWalletCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            mint,
            correlation_id,
        }))]
    }

    fn start_deposit_quote(
        &self,
        ctx: WalletBackendContext<'_>,
        mint: String,
        amount_sats: u64,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        if amount_sats == 0 {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                "deposit amount must be greater than zero".to_string(),
            );
        }
        let accepted = {
            let state = lock_state(&self.state);
            state.mints.iter().any(|m| m == &mint)
        };
        if !accepted {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                "mint not accepted by this wallet".to_string(),
            );
        }
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "deposit-quote");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
            // Pre-effect record: this operation has an HTTP round-trip in
            // flight BEFORE the worker thread (spawned in
            // `CashuDepositQuoteCommand::run`) makes it — this is the
            // "journals ... before the mint request" invariant #2895 W2
            // requires (see `deposit.rs`'s module docs).
            if let Err(e) = state.transition(&operation_id, WalletOperationState::MintPending) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
        }
        vec![ActorCommand::Protocol(Box::new(CashuDepositQuoteCommand {
            state: Arc::clone(&self.state),
            operation_id,
            mint,
            amount_sats,
            correlation_id,
        }))]
    }

    fn start_complete_deposit(
        &self,
        ctx: WalletBackendContext<'_>,
        quote_id: String,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(ui_codes::NO_ACCOUNT, correlation_id, "no active account".to_string());
        };
        let pending = {
            let state = lock_state(&self.state);
            state.pending_deposits.get(&quote_id).cloned()
        };
        let Some(pending) = pending else {
            // Never include the quote_id itself in the failure reason —
            // secret-adjacent (see `state.rs`'s `pending_deposits` docs).
            return fail_closed(
                ui_codes::UNKNOWN_QUOTE,
                correlation_id,
                "no pending deposit for this quote".to_string(),
            );
        };
        vec![ActorCommand::Protocol(Box::new(CashuCompleteDepositCommand {
            state: Arc::clone(&self.state),
            operation_id: pending.operation_id,
            quote_id,
            mint: pending.mint,
            amount_sats: pending.amount_sats,
            account_pubkey,
            correlation_id,
        }))]
    }
}

/// The operation id `start_intent`'s pre-dispatch journal writes key on. Uses
/// the caller's `correlation_id` when present (the normal `dispatch_action`
/// path always mints one) — a fallback timestamp-based id covers direct,
/// non-dispatch callers (tests) and is not guaranteed unique under concurrent
/// no-id calls within the same wall-clock second, which is acceptable since
/// production dispatch always supplies one.
fn operation_id_for(correlation_id: &Option<String>, now_secs: u64, label: &str) -> WalletOperationId {
    match correlation_id {
        Some(id) => WalletOperationId::new(id.clone()),
        None => WalletOperationId::new(format!("cashu-{label}-{now_secs}")),
    }
}

/// Fail-closed before any `ActorCommand::Protocol` is dispatched: a structured
/// `ShowErrorToken` + (when a `correlation_id` was supplied) `RecordActionFailure`.
fn fail_closed(code: &'static str, correlation_id: Option<String>, reason: String) -> Vec<ActorCommand> {
    let mut out = vec![ActorCommand::ShowErrorToken {
        token: UiToken::error(code, reason.clone()),
    }];
    if let Some(id) = correlation_id {
        out.push(ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
            correlation_id: id,
            reason,
        }));
    }
    out
}

/// [`create_wallet::CreateCashuWalletCommand`]'s pure-computation pre-dispatch
/// failure path (e.g. Cashu pubkey derivation) — reported through `ctx`
/// directly since it runs synchronously inside `run`, before any port command
/// is sent.
fn report_pre_dispatch_failure(
    ctx: &ProtocolCommandContext<'_>,
    correlation_id: &Option<String>,
    reason: String,
) {
    let token = UiToken::error(ui_codes::OPERATION_FAILED, reason.clone());
    ctx.set_last_error_token(&token);
    if let Some(id) = correlation_id.clone() {
        ctx.record_action_failure(id, reason);
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
