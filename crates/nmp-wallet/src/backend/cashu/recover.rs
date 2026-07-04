//! `RecoverCashuWallet` intent -> load an account's EXISTING kind:17375
//! wallet config into state (#2965, epic #2864), the counterpart to
//! `create_wallet.rs`'s `CreateCashuWallet` for a returning user whose wallet
//! was published by another client/device (or an earlier install of this
//! one) rather than minted fresh here.
//!
//! This command only ever resolves the account's kind:17375 (Cashu privkey +
//! accepted mints) — see [`RecoverCashuWalletCommand::run`]'s doc comment for
//! why kind:7375 proof recovery is NOT driven from here, and instead always
//! flows through `mod.rs`'s passive `on_self_authored_wallet_event` path
//! (cold-start replay + live tail of the account's own self-authored wallet
//! events), regardless of whether this action ever ran. `ingest.rs` holds
//! the actual decode/fold logic both paths share.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    build_nip44_decrypt_for_account, build_record_action_failure, build_record_action_success,
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::kinds::KIND_NIP60_WALLET;

use super::ingest::ingest_wallet_config;
use super::state::{lock_state, CashuWalletState};
use super::ui_codes;

pub(super) struct RecoverCashuWalletCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) account_pubkey: String,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for RecoverCashuWalletCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoverCashuWalletCommand").finish_non_exhaustive()
    }
}

impl ProtocolCommand for RecoverCashuWalletCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            account_pubkey,
            correlation_id,
        } = *self;

        if lock_state(&state).created {
            // Idempotent: a wallet is already loaded — by a prior
            // `CreateCashuWallet`, a prior `RecoverCashuWallet`, or the
            // passive replay path winning the race. The caller's intent
            // ("make sure I have my existing wallet") is already satisfied.
            if let Some(id) = correlation_id {
                ctx.record_action_success(id, None);
            }
            return Ok(());
        }

        // A point-in-time cache read (see `latest_author_kind`'s doc
        // comment) — there is no "every event for (author, kind)" cached
        // read port, only "latest", which is exactly enough for kind:17375
        // (a single NIP-01 replaceable event) but NOT for enumerating this
        // account's (possibly many) kind:7375 token events. Proof recovery
        // therefore always stays the passive, eventual
        // `CashuWalletBackend::on_self_authored_wallet_event` path — this
        // call's job is only the deterministic config (privkey + mints)
        // yes/no `wallet_self_authored_shape`'s eager cold-start replay
        // (`runtime.rs`) should already have delivered by the time a caller
        // dispatches this action.
        let Some(event) = ctx.latest_author_kind(&account_pubkey, KIND_NIP60_WALLET) else {
            super::report_pre_dispatch_failure(
                ctx,
                &correlation_id,
                ui_codes::NO_EXISTING_WALLET,
                "no existing kind:17375 wallet found on relays for this account".to_string(),
            );
            return Ok(());
        };

        let worker_tx = ctx.command_sender_clone();
        let tx_for_cont = worker_tx.clone();
        let cid_for_send_err = correlation_id.clone();
        let cmd = build_nip44_decrypt_for_account(
            account_pubkey.clone(),
            event.content,
            Some(account_pubkey),
            move |outcome| {
                let plaintext = match outcome {
                    Ok(p) => p,
                    Err(reason) => {
                        report_failure(
                            &tx_for_cont,
                            correlation_id,
                            format!("nip44 self-decrypt: {reason}"),
                        );
                        return;
                    }
                };
                match ingest_wallet_config(&state, &plaintext) {
                    Ok(()) => {
                        if let Some(id) = correlation_id {
                            let _ = tx_for_cont.send(build_record_action_success(id, None));
                        }
                    }
                    Err(reason) => report_failure(&tx_for_cont, correlation_id, reason),
                }
            },
        );
        if worker_tx.send(cmd).is_err() {
            report_failure(
                &worker_tx,
                cid_for_send_err,
                "actor inbox closed before nip44 self-decrypt".to_string(),
            );
        }
        Ok(())
    }
}

fn report_failure(worker_tx: &CommandSender, correlation_id: Option<String>, reason: String) {
    let token = UiToken::error(ui_codes::OPERATION_FAILED, reason.clone());
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id {
        let _ = worker_tx.send(build_record_action_failure(id, reason));
    }
}
