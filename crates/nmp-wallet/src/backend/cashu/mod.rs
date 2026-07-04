//! The Cashu [`WalletBackend`] adapter (#2895 W2, epic #2864) — the second
//! concrete backend behind the seam, alongside `backend::nwc`.
//!
//! This adapter owns:
//!
//! * `capabilities()` advertises `cashu_nutzaps()` (#2917 — create/deposit
//!   plus publish-info/send/redeem nutzap, and `observe_nutzap_receipts`; see
//!   `WalletCapabilities::cashu_nutzaps`'s doc comment).
//! * `start_intent()` maps every `WalletIntent` this backend implements onto
//!   `ActorCommand::Protocol` commands (`create_wallet.rs`, `deposit.rs`,
//!   `publish_info.rs`, `send.rs`, `redeem.rs`) that drive the
//!   signer-transparent NIP-44 + sign ports and `nmp-nip60`'s mint HTTP lane.
//!   `RecoverCashuWallet`/`MeltCashu` remain unimplemented, so `start_intent`
//!   is a documented no-op for those (never a panic — D6), same as
//!   `backend::nwc` treats its own out-of-capability intents.
//! * `snapshot()` reads the shared [`state::CashuWalletState`] this backend's
//!   own commands/workers write (D4: this backend's commands are the sole
//!   writer, mirroring `NwcWalletBackend`'s `WalletStatusSlot`), including
//!   (#2949) `recent_history`/`receive_rows` folded from the journal's
//!   terminal operations — see `snapshot.rs`.
//!
//! # Live wiring (epic #2864 W7, #2908) vs. still-deferred behavior
//!
//! `crate::register::register` now constructs this backend, registers it in
//! the `WalletBackendSelector`, and (via `crate::runtime::WalletRuntime`)
//! routes every observed kind:9321/10019/17375/7375/7376/7374 `KernelEvent`
//! into `on_wallet_event`. As of #2917, `on_wallet_event` acts on kind:9321
//! (dispatches `redeem::RedeemNutzapCommand` — see that method's doc comment)
//! but still no-ops kind:17375/7375/7376/10019: reconciling THOSE durable
//! events into `WalletLedger::rebuild_from` on cold start needs a
//! signer-mediated NIP-44 decrypt this backend does not yet perform (this
//! backend's in-memory state today is populated only by its own
//! `start_intent`-driven operations, plus whatever `on_wallet_event` folds
//! from observed kind:9321s). A documented no-op, not a silent partial read.
//! * `on_mint_result` — this backend's own `ProtocolCommand` workers
//!   (`CashuDepositQuoteCommand`, `CashuCompleteDepositCommand`, and #2917's
//!   send/redeem workers) own the full mint-round-trip -> state/publish
//!   mapping directly against the richer data the mint response actually
//!   carries (quote id, bolt11, proofs); the coarse `MintResult` seam
//!   (operation_id + Settled/Failed/Unknown) cannot carry that shape, and
//!   nothing constructs one for this backend today. Mirrors
//!   `NwcWalletBackend::on_mint_result`'s treatment of the same seam for the
//!   opposite reason ("not this backend's concept").

mod chain;
mod create_wallet;
mod deposit;
mod nutzap_dispatch;
mod publish_info;
mod redeem;
mod redeem_worker;
mod send;
mod send_worker;
mod snapshot;
mod state;
mod ui_codes;

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{KernelEvent, ProtocolCommandContext};
use nmp_core::ui_token::UiToken;

use crate::fail_closed::fail_closed;
use crate::journal::{WalletOperationId, WalletOperationKind, WalletOperationState};
use crate::projection::{WalletBalanceRow, WalletProjection, WalletReadiness};

use super::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
use crate::capability::WalletCapabilities;

use create_wallet::CreateCashuWalletCommand;
use deposit::{CashuCompleteDepositCommand, CashuDepositQuoteCommand};
use nutzap_dispatch::redeem_operation_id;
use redeem::RedeemNutzapCommand;
use state::{canonicalize_mint_url, is_well_formed_mint_url, lock_state, CashuWalletState};

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

    /// Discard all in-memory wallet state (created flag, mints, Cashu P2PK
    /// pubkey, ledger, journal, pending deposits) and start fresh.
    ///
    /// Required because this backend, unlike `NwcWalletBackend`'s connection
    /// state, holds NIP-44-encrypted-to-a-specific-identity material
    /// (`kind:17375`'s Cashu private key + accepted mints, `kind:7375`
    /// proofs): it is constructed once per app instance
    /// (`register.rs`), not once per signed-in account, so without this reset
    /// a Nostr account switch within one running app would leave the
    /// PREVIOUS account's balance/mint list/pending deposits visible to
    /// (and, via `complete_deposit`, completable as) the NEWLY active
    /// account — a cross-account data/fund leak. Callers wire this to fire
    /// on every active-account change (`nmp_core::substrate::IdentityChangeRegistrar`),
    /// mirroring how `nmp-nip51`'s `MuteListProjection` resets on the same
    /// signal. Losing durable wallet history on account switch is expected
    /// and safe: nothing here is the source of truth — the durable
    /// `kind:17375`/`kind:7375`/`kind:7376` events are — cold-start
    /// reconciliation from that event stream back into this state is a
    /// separate, already-documented deferral (see `on_wallet_event`'s doc
    /// comment on this backend), not something this reset regresses.
    pub fn reset(&self) {
        *lock_state(&self.state) = CashuWalletState::new();
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
        WalletCapabilities::cashu_nutzaps()
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
        let terminal = state.journal.terminal_operations();
        projection = projection
            .with_recent_history(snapshot::recent_history(&state, &terminal))
            .with_receive_rows(snapshot::receive_rows(&terminal));
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
            WalletIntent::DepositQuoteCashu { mint, amount_sats } => {
                self.start_deposit_quote(ctx, mint, amount_sats, correlation_id)
            }
            WalletIntent::CompleteDepositCashu { quote_id } => {
                self.start_complete_deposit(ctx, quote_id, correlation_id)
            }
            WalletIntent::PublishNutzapInfo => self.start_publish_nutzap_info(ctx, correlation_id),
            WalletIntent::SendNutzap {
                recipient_pubkey,
                amount_sats,
                target_event_id,
            } => self.start_send_nutzap(
                ctx,
                recipient_pubkey,
                amount_sats,
                target_event_id,
                correlation_id,
            ),
            WalletIntent::RedeemNutzap { event_id } => {
                self.start_redeem_nutzap(ctx, event_id, correlation_id)
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
            | WalletIntent::MeltCashu { .. } => Vec::new(),
        }
    }

    fn on_wallet_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        // #2917 (W9) — a received nutzap. Everything else (kind:17375/7375/
        // 7376/10019 cold-start reconciliation) stays the documented no-op
        // from the module doc comment.
        if event.kind != nmp_nip60::kinds::KIND_NIP61_NUTZAP {
            return Vec::new();
        }
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return Vec::new();
        };
        // Keyed by the nutzap's own event id (not a correlation id — this
        // path has none): re-observing the same kind:9321 (a relay resend,
        // or an explicit `nmp.wallet.nutzap.redeem` retry for one the
        // observer already started) hits `DuplicateOperation` here and is
        // silently skipped rather than double-dispatched — a natural
        // at-most-once guard shared with `start_redeem_nutzap` below.
        let operation_id = redeem_operation_id(&event.id);
        {
            let mut state = lock_state(&self.state);
            if state
                .begin_operation(operation_id.clone(), WalletOperationKind::RedeemNutzap)
                .is_err()
            {
                return Vec::new();
            }
        }
        vec![ActorCommand::Protocol(Box::new(RedeemNutzapCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            event_id: event.id.clone(),
            correlation_id: None,
        }))]
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
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        if !is_well_formed_mint_url(&mint) {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                format!("unsupported mint: {mint}"),
            );
        }
        // Fail closed rather than silently re-creating: a second wallet
        // event would overwrite `mints`/`cashu_pubkey_hex` for a wallet that
        // may already hold ledger balance under the first mint. This does
        // not close the narrower "two `CreateCashuWallet` calls dispatched
        // back-to-back before the first one's async chain finishes" race
        // (`created` only flips once `on_signed` runs) — that needs the
        // higher dispatch layer's own dedup, same as `nmp-nip47`'s
        // `nmp.wallet.pay_invoice` action rejects a same-invoice retap in
        // `start()` (see `nwc.rs`'s doc comment).
        if lock_state(&self.state).created {
            return fail_closed(
                ui_codes::ALREADY_CREATED,
                correlation_id,
                "wallet already created".to_string(),
            );
        }
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "create");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) =
                state.begin_operation(operation_id.clone(), WalletOperationKind::CreateCashuWallet)
            {
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
            // #2972 — compare canonically: the mint typed for THIS deposit
            // need not be byte-identical to the string this wallet's
            // `mints` allow-list was created with (trailing slash,
            // scheme/host case) to be the same real mint.
            state
                .mints
                .iter()
                .any(|m| canonicalize_mint_url(m) == canonicalize_mint_url(&mint))
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
            if let Err(e) =
                state.begin_operation(operation_id.clone(), WalletOperationKind::DepositCashu)
            {
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
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
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
        vec![ActorCommand::Protocol(Box::new(
            CashuCompleteDepositCommand {
                state: Arc::clone(&self.state),
                operation_id: pending.operation_id,
                quote_id,
                mint: pending.mint,
                amount_sats: pending.amount_sats,
                account_pubkey,
                correlation_id,
            },
        ))]
    }

}

/// The operation id `start_intent`'s pre-dispatch journal writes key on. Uses
/// the caller's `correlation_id` when present (the normal `dispatch_action`
/// path always mints one) — a fallback timestamp-based id covers direct,
/// non-dispatch callers (tests) and is not guaranteed unique under concurrent
/// no-id calls within the same wall-clock second, which is acceptable since
/// production dispatch always supplies one.
fn operation_id_for(
    correlation_id: &Option<String>,
    now_secs: u64,
    label: &str,
) -> WalletOperationId {
    match correlation_id {
        Some(id) => WalletOperationId::new(id.clone()),
        None => WalletOperationId::new(format!("cashu-{label}-{now_secs}")),
    }
}

/// A `ProtocolCommand::run` body's own pure-computation or validation failure
/// path (e.g. Cashu pubkey derivation, an unresolvable recipient/relay set)
/// — reported through `ctx` directly since it runs synchronously inside
/// `run`, before (or instead of) any port/worker-thread command is sent.
/// `code` is the specific `ui_codes` constant for this failure (never the
/// generic `OPERATION_FAILED` — #2917's send/redeem/publish-info commands
/// each have several distinguishable fail-closed reasons a caller should be
/// able to tell apart).
fn report_pre_dispatch_failure(
    ctx: &ProtocolCommandContext<'_>,
    correlation_id: &Option<String>,
    code: &'static str,
    reason: String,
) {
    let token = UiToken::error(code, reason.clone());
    ctx.set_last_error_token(&token);
    if let Some(id) = correlation_id.clone() {
        ctx.record_action_failure(id, reason);
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
