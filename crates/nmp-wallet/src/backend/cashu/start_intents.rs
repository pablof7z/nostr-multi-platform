//! The per-intent `start_*` dispatch helpers for [`CashuWalletBackend`] —
//! split out of `mod.rs` (AGENTS.md file-size discipline, mirroring the
//! `send.rs`/`send_worker.rs` split). `mod.rs` keeps the `WalletBackend`
//! trait impl (including `start_intent`'s intent→helper match); this file
//! holds the cohesive `impl CashuWalletBackend` block those match arms call
//! into.
//!
//! Each helper does the synchronous, pre-dispatch fail-closed validation +
//! journal write for one `WalletIntent`, then returns the `ProtocolCommand`
//! (or a `fail_closed` `UiToken`) the actor runs. The commands themselves
//! live in the sibling `create_wallet.rs`/`deposit.rs`/`recover.rs`/
//! `set_mints.rs` files.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;

use crate::backend::WalletBackendContext;
use crate::fail_closed::fail_closed;
use crate::journal::{WalletOperationKind, WalletOperationState};

use super::create_wallet::CreateCashuWalletCommand;
use super::cross_mint_worker::SendRetry;
use super::deposit::{CashuCompleteDepositCommand, CashuDepositQuoteCommand};
use super::recover::RecoverCashuWalletCommand;
use super::set_mints::SetCashuMintsCommand;
use super::state::{canonicalize_mint_url, is_well_formed_mint_url, lock_state};
use super::{operation_id_for, ui_codes, CashuWalletBackend};

impl CashuWalletBackend {
    pub(super) fn start_create_wallet(
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
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::CreateCashuWallet,
                ctx.now_secs,
            ) {
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

    pub(super) fn start_deposit_quote(
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
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::DepositCashu,
                ctx.now_secs,
            ) {
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

    pub(super) fn start_complete_deposit(
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

    /// #2965 — the explicit `nmp.wallet.cashu.recover` action path. See
    /// `recover.rs`'s module docs for why only the kind:17375 config is
    /// resolved deterministically here, with proof recovery left to the
    /// passive `on_self_authored_wallet_event` path regardless.
    pub(super) fn start_recover_wallet(
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
        vec![ActorCommand::Protocol(Box::new(
            RecoverCashuWalletCommand {
                state: Arc::clone(&self.state),
                account_pubkey,
                correlation_id,
            },
        ))]
    }

    /// #2997 — `nmp.wallet.cashu.set_mints`: replace the accepted-mint list
    /// while carrying the EXISTING Cashu P2PK privkey forward unchanged (never
    /// `WalletConfig::generate`s a fresh one — see `set_mints.rs`'s module
    /// docs for why rotating it here would strand already-incoming
    /// P2PK-locked proofs). Fails closed when no Cashu wallet has been
    /// created/recovered yet: there is no existing privkey to carry forward,
    /// and this action must never silently fall back to minting a fresh one
    /// (that would be `cashu.create`'s job, not this one's).
    pub(super) fn start_set_mints(
        &self,
        ctx: WalletBackendContext<'_>,
        mints: Vec<String>,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        if mints.is_empty() {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                "cashu.set_mints requires a non-empty mint list".to_string(),
            );
        }
        if let Some(bad) = mints.iter().find(|m| !is_well_formed_mint_url(m)) {
            return fail_closed(
                ui_codes::UNSUPPORTED_MINT,
                correlation_id,
                format!("unsupported mint: {bad}"),
            );
        }
        // Fail closed when this wallet has never been created/recovered:
        // there is no existing Cashu P2PK privkey to carry forward, and this
        // action must never mint a fresh one (that is `cashu.create`'s job).
        let has_wallet = {
            let state = lock_state(&self.state);
            state.cashu_privkey.is_some() && state.cashu_pubkey_hex.is_some()
        };
        if !has_wallet {
            return fail_closed(
                ui_codes::NO_CASHU_WALLET,
                correlation_id,
                "no Cashu wallet exists yet; use cashu.create or cashu.recover first".to_string(),
            );
        }
        let operation_id = operation_id_for(&correlation_id, ctx.now_secs, "set-mints");
        {
            let mut state = lock_state(&self.state);
            if let Err(e) = state.begin_operation_at(
                operation_id.clone(),
                WalletOperationKind::SetCashuMints,
                ctx.now_secs,
            ) {
                return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
            }
        }
        vec![ActorCommand::Protocol(Box::new(SetCashuMintsCommand {
            state: Arc::clone(&self.state),
            operation_id,
            account_pubkey,
            mints,
            correlation_id,
        }))]
    }

    /// #3003 — cross-mint nutzap funding. Fund `target_mint` with
    /// `amount_sats` by melting proofs at whichever OTHER mint holds the
    /// largest spendable balance (see
    /// `state_cross_mint::largest_spendable_mint_excluding`'s doc comment).
    /// Called both by the standalone `nmp.wallet.cashu.cross_mint_transfer`
    /// action (`on_settled: None`) and internally by `send.rs`'s
    /// `nutzap.send` auto-fallback (`on_settled: Some(_)` — re-dispatches
    /// the original send once the transfer settles). Delegates the
    /// validation + selection + journal pre-record shared by both callers to
    /// `cross_mint::build_cross_mint_transfer`.
    pub(super) fn start_cross_mint_transfer(
        &self,
        ctx: WalletBackendContext<'_>,
        target_mint: String,
        amount_sats: u64,
        correlation_id: Option<String>,
        on_settled: Option<SendRetry>,
    ) -> Vec<ActorCommand> {
        let Some(account_pubkey) = ctx.account_pubkey.map(str::to_string) else {
            return fail_closed(
                ui_codes::NO_ACCOUNT,
                correlation_id,
                "no active account".to_string(),
            );
        };
        super::cross_mint::build_cross_mint_transfer(
            Arc::clone(&self.state),
            account_pubkey,
            ctx.now_secs,
            target_mint,
            amount_sats,
            correlation_id,
            on_settled,
        )
    }
}
