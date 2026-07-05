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
//!   `publish_info.rs`, `send.rs`, `redeem.rs`, `recover.rs`, `set_mints.rs`)
//!   that drive the signer-transparent NIP-44 + sign ports and `nmp-nip60`'s
//!   mint HTTP lane.
//!   `MeltCashu` remains unimplemented, so `start_intent` is a documented
//!   no-op for it (never a panic — D6), same as `backend::nwc` treats its own
//!   out-of-capability intents.
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
//! (dispatches `redeem::RedeemNutzapCommand` — see that method's doc comment).
//! As of #2965, it ALSO acts on the account's own self-authored kind:17375/
//! 7375 (`on_self_authored_wallet_event`, below): a signer-mediated NIP-44
//! decrypt (`ingest.rs`) loads an existing wallet's config/proofs into this
//! backend's in-memory state, rather than leaving a returning user's
//! already-published wallet invisible behind a fresh `CreateCashuWallet`.
//! kind:7376/10019 stay a documented no-op — history replay and the
//! account's own nutzap-info cache are not this backend's concern (the
//! former is display-only, the latter is read on demand via
//! `ctx.latest_author_kind` where needed, e.g. `publish_info.rs`).
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
mod check_state;
mod create_wallet;
mod cross_mint;
mod cross_mint_publish;
mod cross_mint_resume;
mod cross_mint_worker;
mod deposit;
mod events;
mod ingest;
mod lifecycle;
mod nutzap_await;
mod nutzap_dispatch;
mod publish_info;
mod recover;
mod redeem;
mod redeem_worker;
mod send;
mod send_worker;
mod set_mints;
mod snapshot;
mod start_intents;
mod state;
mod ui_codes;
mod wal_payload;
mod wal_redeem;
mod wal_send;

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{KernelEvent, ProtocolCommandContext};
use nmp_core::ui_token::UiToken;

use crate::journal::{WalletOperationId, WalletWalStore};
use crate::projection::{WalletBalanceRow, WalletProjection, WalletReadiness};

use super::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
use crate::capability::WalletCapabilities;

use state::{lock_state, CashuWalletState};

// Re-exported (not just imported) so `action::cashu::CashuSetMintsModule::start`
// can validate each mint URL BEFORE dispatch, mirroring exactly the gate
// `start_create_wallet` below applies at the backend layer — the `state`
// submodule itself stays private, only this one helper is elevated.
pub(crate) use state::is_well_formed_mint_url;

/// Canonical id this backend registers under.
pub const CASHU_BACKEND_ID: &str = "cashu";

/// [`WalletBackend`] adapter over `nmp-nip60`'s wallet-config codec + mint
/// HTTP lane.
pub struct CashuWalletBackend {
    state: Arc<Mutex<CashuWalletState>>,
    /// The durable pre-publish WAL store (PR-1 of #2910/#2960/#2931), or `None`
    /// for in-memory-only. Held here — not only inside `state` — because
    /// [`Self::reset`] replaces the whole `CashuWalletState` on every account
    /// switch, so the app-lifetime store has to be re-threaded into the fresh
    /// state each time from this field, which survives the reset.
    wal_store: Option<Arc<dyn WalletWalStore>>,
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
            WalletIntent::RecoverCashuWallet => self.start_recover_wallet(ctx, correlation_id),
            WalletIntent::SetCashuMints { mints } => {
                self.start_set_mints(ctx, mints, correlation_id)
            }
            WalletIntent::CrossMintTransfer {
                target_mint,
                amount_sats,
            } => self.start_cross_mint_transfer(
                ctx,
                target_mint,
                amount_sats,
                correlation_id,
                // A direct `WalletIntent::CrossMintTransfer` dispatch (the
                // explicit action, or a test) has no original send to
                // re-drive — only `send.rs`'s internal auto-fallback
                // constructs a `SendRetry`.
                None,
            ),
            // Not this backend's capability — `capabilities()` already tells
            // callers not to route these here. A no-op rather than a panic
            // keeps a stray dispatch harmless (D6), same as `backend::nwc`.
            WalletIntent::SelectBackend { .. }
            | WalletIntent::PayBolt11 { .. }
            | WalletIntent::MeltCashu { .. } => Vec::new(),
        }
    }

    fn on_wallet_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        if event.kind == nmp_nip60::kinds::KIND_NIP61_NUTZAP {
            return self.on_nutzap_event(ctx, event);
        }
        if event.kind == nmp_nip60::kinds::KIND_NIP60_WALLET
            || event.kind == nmp_nip60::kinds::KIND_NIP60_TOKEN
        {
            return self.on_self_authored_wallet_event(ctx, event);
        }
        // kind:7376/10019 stay the documented no-op — see the module doc
        // comment.
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
