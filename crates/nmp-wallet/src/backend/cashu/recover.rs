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
//!
//! # Check-state before reporting success (#2977)
//!
//! Both the idempotent ("already loaded") branch and the happy-path decrypt
//! chain defer `RecordActionSuccess` until AFTER
//! `check_state::run_check_state_pass` has reconciled whatever proofs are
//! currently held against their mints — this is the one caller of that pass
//! that "awaits" it rather than firing it silently: a caller polling balance
//! right after `nmp.wallet.cashu.recover` returns must see the
//! mint-reconciled, UNSPENT-only figure, not a transiently optimistic one.
//! Both branches spawn their own `std::thread` for this (D8 — mint HTTP
//! never blocks the actor thread), mirroring every other worker in this
//! backend.

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

use super::check_state::run_check_state_pass;
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
            // ("make sure I have my existing wallet") is already satisfied
            // for the config; still worth reconciling whatever proofs are
            // already held against their mints before reporting success
            // (#2977 — see module docs) rather than assuming the passive
            // path's own check-state pass already ran.
            let worker_tx = ctx.command_sender_clone();
            // #3030 PR2 of 2 — "once on recover": an explicit recover call
            // (even this idempotent, already-loaded branch) is exactly the
            // "make sure my wallet's info is up to date" moment a shell
            // dispatches this action for — refresh cached NUT-06/NUT-02 info
            // for the currently-accepted mints too.
            let mints = lock_state(&state).mints.clone();
            super::mint_info::spawn_mint_info_refresh(Arc::clone(&state), mints);
            std::thread::spawn(move || {
                run_check_state_pass(&state);
                if let Some(id) = correlation_id {
                    let _ = worker_tx.send(build_record_action_success(id, None));
                }
            });
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
                    Ok(fresh) => {
                        // #3030 PR2 of 2 — this is the "once on recover"
                        // trigger for a wallet that had NOT been loaded yet
                        // (the already-loaded idempotent branch above has its
                        // own trigger). `fresh` is always `true` on this path
                        // in practice (the caller already checked `!created`
                        // before reaching here), but this reads the return
                        // value rather than assuming it, matching
                        // `ingest.rs`'s passive-path caller.
                        if fresh {
                            let mints = lock_state(&state).mints.clone();
                            super::mint_info::spawn_mint_info_refresh(
                                Arc::clone(&state),
                                mints,
                            );
                        }
                        // #2977 — reconcile before reporting success (see
                        // module docs); off the actor thread, this
                        // continuation's own thread since mint HTTP is
                        // blocking (D8).
                        std::thread::spawn(move || {
                            run_check_state_pass(&state);
                            if let Some(id) = correlation_id {
                                let _ = tx_for_cont.send(build_record_action_success(id, None));
                            }
                        });
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
