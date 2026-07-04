//! `PublishNutzapInfo`/`SendNutzap`/`RedeemNutzap` dispatch — split out of
//! `mod.rs` (AGENTS.md file-size discipline). An `impl CashuWalletBackend`
//! extension block; `mod.rs`'s `start_intent` match calls these three
//! methods directly.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;

use crate::fail_closed::fail_closed;
use crate::journal::{WalletOperationId, WalletOperationKind};

use super::publish_info::PublishNutzapInfoCommand;
use super::redeem::RedeemNutzapCommand;
use super::send::SendNutzapCommand;
use super::state::lock_state;
use super::{operation_id_for, ui_codes, CashuWalletBackend, WalletBackendContext};

impl CashuWalletBackend {
    /// #2917 (W13) — `PublishNutzapInfo`: no pre-effect journal state (this
    /// never consumes proofs), so `begin_operation` is the only pre-dispatch
    /// work before handing off to `PublishNutzapInfoCommand`, which resolves
    /// relays and builds/signs/publishes kind:10019.
    pub(super) fn start_publish_nutzap_info(
        &self,
        ctx: WalletBackendContext<'_>,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "publish-info");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::PublishNutzapInfo,
                ctx.now_secs,
            ) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
        }
        vec![ActorCommand::Protocol(Box::new(
            PublishNutzapInfoCommand {
                state: Arc::clone(&self.state),
                operation_id,
                account_pubkey,
                correlation_id,
            },
        ))]
    }

    /// #2917 (W8) — `SendNutzap`: recipient resolution, mint/proof selection,
    /// and the mint swap all need a live `ProtocolCommandContext`/worker
    /// thread, so (mirroring `start_deposit_quote`) only the account gate and
    /// journal pre-record happen here; `SendNutzapCommand::run` does the rest
    /// (including the fail-closed recipient-info/mint/P2PK/relay/balance
    /// checks — see that file's module docs).
    pub(super) fn start_send_nutzap(
        &self,
        ctx: WalletBackendContext<'_>,
        recipient_pubkey: String,
        amount_sats: u64,
        target_event_id: Option<String>,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "send-nutzap");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::SendNutzap,
                ctx.now_secs,
            ) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
            // #2966 — the intended send amount is known right now, before
            // recipient/mint/proof resolution ever runs, and is the correct
            // history-row display amount even if every later step fails
            // (`consumed_inputs` only gets populated once proofs are
            // actually selected — see `snapshot.rs`'s `history_row`).
            let _ = state.journal.record_amount(&operation_id, amount_sats);
        }
        vec![ActorCommand::Protocol(Box::new(SendNutzapCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            recipient_pubkey,
            amount_sats,
            target_event_id,
            correlation_id,
        }))]
    }

    /// #2917 (W9) — the explicit `nmp.wallet.nutzap.redeem` action path.
    /// Shares [`redeem_operation_id`] with `on_wallet_event`'s observer path
    /// (see that method's doc comment in `mod.rs`) so redeeming an event
    /// twice — whether the observer already started it or a caller retaps
    /// the action — hits `DuplicateOperation` here and fails closed rather
    /// than double-spending.
    pub(super) fn start_redeem_nutzap(
        &self,
        ctx: WalletBackendContext<'_>,
        event_id: String,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        let operation_id = redeem_operation_id(&event_id);
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::RedeemNutzap,
                ctx.now_secs,
            ) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
        }
        vec![ActorCommand::Protocol(Box::new(RedeemNutzapCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            event_id,
            correlation_id,
        }))]
    }
}

/// The journal operation id every `RedeemNutzap` path (the explicit action
/// and `mod.rs`'s `on_wallet_event` observer) keys on — the nutzap's own
/// event id, not a correlation id, so the SAME kind:9321 can only ever have
/// one in-flight/completed redeem operation (see both call sites' doc
/// comments).
pub(super) fn redeem_operation_id(event_id: &str) -> WalletOperationId {
    WalletOperationId::new(format!("redeem-{event_id}"))
}
