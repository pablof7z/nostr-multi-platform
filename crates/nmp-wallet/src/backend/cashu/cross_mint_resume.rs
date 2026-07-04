//! Cross-mint transfer (#3003) durable WAL write-through + cold-restart
//! resume — mirrors `wal_send.rs`'s combined persist/restore/`Resume*Command`
//! shape. See `cross_mint`'s module docs for the full saga; this file owns
//! only "how a crash is durably recorded and reconciled on restart", never
//! the live melt/mint HTTP calls themselves (those live in
//! `cross_mint_worker.rs`).

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_core::ui_token::UiToken;
use nmp_nip60::cashu::MintClient;

use crate::journal::{WalletOperationKind, WalletOperationState, WalletWalStore};

use super::state::{lock_state, CashuWalletState, PendingCrossMintTransfer};
use super::ui_codes;
use super::wal_payload::{CashuWalPayload, StoredProofRecord};

/// Write the current cross-mint-transfer resume payload for
/// `target_quote_id` through to the durable WAL. A no-op when no WAL/account
/// is configured (in-memory-only parity) or the transfer is unknown.
/// Failures are swallowed — the payload is a durability shadow and must
/// never fail the in-memory mutation that already succeeded (D6), exactly
/// like `wal_payload.rs`'s deposit/send variants.
///
/// Call this at every cross-mint-transfer write point, AFTER mutating the
/// in-memory [`PendingCrossMintTransfer`], so the payload always reflects
/// the same state a resume would read: reserved (melt not yet attempted),
/// melt settled, minted, signed.
pub(super) fn persist_cross_mint_payload(state: &CashuWalletState, target_quote_id: &str) {
    let (Some(store), Some(account)) = (state.wal_store.as_ref(), state.wal_account.as_ref())
    else {
        return;
    };
    let Some(pending) = state.pending_cross_mint_transfers.get(target_quote_id) else {
        return;
    };
    let payload = CashuWalPayload::CrossMintTransfer {
        target_quote_id: pending.target_quote_id.clone(),
        target_mint: pending.target_mint.clone(),
        source_mint: pending.source_mint.clone(),
        amount_sats: pending.amount_sats,
        melt_quote_id: pending.melt_quote_id.clone(),
        source_selected: pending
            .source_selected
            .iter()
            .map(StoredProofRecord::from)
            .collect(),
        melt_settled: pending.melt_settled,
        minted_proofs: pending.minted_proofs.clone(),
        signed_token: pending.signed_token.clone(),
    };
    if let Some(bytes) = payload.encode() {
        let _ = store.upsert_payload(account, &pending.operation_id, &bytes);
    }
}

/// A restored cross-mint transfer still needing an attempt (reconcile the
/// melt, resume the target mint, or resume publish) — returned for EVERY
/// non-terminal `CrossMintTransfer` operation found (unlike deposit's
/// `ResumeDeposit`, which only re-drives past-the-mint entries): if this
/// entry exists at all in the durable WAL, the operation is not `Settled`,
/// so SOME further step is always needed.
pub(super) struct ResumeCrossMintTransfer {
    pub(super) target_quote_id: String,
}

/// Rebuild `pending_cross_mint_transfers` from the durable WAL for `account`
/// and report which transfers still need an attempt. Called from
/// `restore_from_wal` AFTER `restore_into_journal` has rehydrated the saga
/// journal and self-healed terminal rows, so every op read back here is one
/// whose lookup a `ResumeCrossMintTransferCommand` must be able to satisfy.
pub(super) fn restore_cross_mint_transfers(
    state: &mut CashuWalletState,
    store: &dyn WalletWalStore,
    account: &str,
) -> Vec<ResumeCrossMintTransfer> {
    let mut resumes = Vec::new();
    let Ok(operations) = store.load_operations(account) else {
        return resumes;
    };
    for op in operations {
        if op.kind != WalletOperationKind::CrossMintTransfer || op.state.is_terminal() {
            continue;
        }
        let Ok(Some(bytes)) = store.load_payload(account, &op.id) else {
            continue;
        };
        let Some(CashuWalPayload::CrossMintTransfer {
            target_quote_id,
            target_mint,
            source_mint,
            amount_sats,
            melt_quote_id,
            source_selected,
            melt_settled,
            minted_proofs,
            signed_token,
        }) = CashuWalPayload::decode(&bytes)
        else {
            continue;
        };
        state.pending_cross_mint_transfers.insert(
            target_quote_id.clone(),
            PendingCrossMintTransfer {
                operation_id: op.id.clone(),
                target_mint,
                source_mint,
                amount_sats,
                target_quote_id: target_quote_id.clone(),
                melt_quote_id,
                source_selected: source_selected
                    .into_iter()
                    .map(StoredProofRecord::into_stored)
                    .collect(),
                melt_settled,
                minted_proofs,
                signed_token,
                // A lease is a transient, in-flight-attempt token — never
                // persisted, mirrors `restore_deposits`.
                chain_started_at: None,
            },
        );
        resumes.push(ResumeCrossMintTransfer { target_quote_id });
    }
    resumes
}

pub(super) struct ResumeCrossMintTransferCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) account_pubkey: String,
    pub(super) target_quote_id: String,
}

impl fmt::Debug for ResumeCrossMintTransferCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeCrossMintTransferCommand")
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for ResumeCrossMintTransferCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            account_pubkey,
            target_quote_id,
        } = *self;
        // D7 — re-stamp the wall clock before the kind:7375 is (re)built;
        // relays resolved here (same seam `ResumeDepositCommand` uses) since
        // the spawned worker below has no live `ctx`.
        let created_at = ctx.now_secs();
        let relays = ctx.recipient_publish_relays(&account_pubkey, nmp_nip60::KIND_NIP60_TOKEN);
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let pending = {
                let mut guard = lock_state(&state);
                let Some(pending) = guard.pending_cross_mint_transfers.get_mut(&target_quote_id)
                else {
                    // Settled/removed between restore and this command
                    // running — nothing left to re-drive.
                    return;
                };
                pending.chain_started_at = Some(created_at);
                pending.clone()
            };

            if !pending.melt_settled {
                // The melt's true outcome was never confirmed before the
                // crash — reconcile via the SOURCE mint's own melt-quote
                // status rather than assuming success or failure (the
                // money-safety invariant #3003 exists to close).
                let client = MintClient::new(&pending.source_mint);
                match client.get_melt_quote_status(&pending.melt_quote_id) {
                    Ok(status) if status.state == nmp_nip60::cashu::types::MeltQuoteState::Paid => {
                        {
                            let mut guard = lock_state(&state);
                            if let Some(p) =
                                guard.pending_cross_mint_transfers.get_mut(&target_quote_id)
                            {
                                p.melt_settled = true;
                            }
                            let _ = guard.transition(
                                &pending.operation_id,
                                WalletOperationState::MintSettled,
                            );
                            super::cross_mint_resume::persist_cross_mint_payload(
                                &guard,
                                &target_quote_id,
                            );
                        }
                        super::cross_mint_publish::resume_target_mint_leg(
                            worker_tx,
                            state,
                            account_pubkey,
                            target_quote_id,
                            relays,
                            created_at,
                            None,
                            None,
                        );
                    }
                    Ok(_unpaid_or_expired) => {
                        // The melt never happened — safe to restore the
                        // reserved source proofs (nothing external consumed
                        // them) and fail the operation closed. NOTE: any
                        // fee-reserve change from a melt that DID commit
                        // between the last live check and this restart is
                        // covered by `Paid` above, not this branch.
                        let mut guard = lock_state(&state);
                        for stored in &pending.source_selected {
                            guard.add_proofs(
                                stored.token_event.clone(),
                                pending.source_mint.clone(),
                                vec![stored.proof.clone()],
                            );
                        }
                        guard.pending_cross_mint_transfers.remove(&target_quote_id);
                        let _ =
                            guard.transition(&pending.operation_id, WalletOperationState::Failed);
                        drop(guard);
                        // No live correlation id to resolve on a cold-restart
                        // resume (mirrors `ResumeSendCommand`/
                        // `ResumeDepositCommand`) — still surface a UI error
                        // token so an observing shell learns the transfer
                        // reverted, rather than a silent state change.
                        let token = UiToken::error(
                            ui_codes::MELT_FAILED,
                            "cross-mint transfer's melt was never paid; reserved source proofs restored".to_string(),
                        );
                        let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
                    }
                    Err(_) => {
                        // Transport failure reconciling — leave everything
                        // exactly as-is (still `melt_settled: false`); the
                        // next restart/resume tries again. Never restore,
                        // never assume paid.
                    }
                }
                return;
            }

            // The melt already settled pre-crash — resume straight into the
            // target-mint leg (retry-safe: `mint_tokens` against a PAID,
            // not-yet-ISSUED quote is idempotent, and `minted_proofs`'s
            // write-if-absent fence — mirroring `PendingDeposit` — means a
            // concurrent resume can never double-mint).
            super::cross_mint_publish::resume_target_mint_leg(
                worker_tx,
                state,
                account_pubkey,
                target_quote_id,
                relays,
                created_at,
                None,
                None,
            );
        });
        Ok(())
    }
}
