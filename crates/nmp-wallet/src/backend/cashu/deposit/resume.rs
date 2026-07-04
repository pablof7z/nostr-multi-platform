//! [`ResumeDepositCommand`] — the cold-start re-drive of a deposit rebuilt from
//! the durable WAL (PR-2 of #2910). `restore_from_wal` repopulates
//! `pending_deposits` from persisted payloads (see
//! [`super::super::wal_payload::restore_deposits`]) and enqueues one of these
//! per deposit that still needs its encrypt/sign/publish chain finished. This
//! command runs the SAME two chain tails the in-process `DepositResume` retry
//! path (`super::complete`) does — it does not duplicate the encrypt/sign
//! logic:
//!
//! - `signed_token` set: republish the EXACT cached kind:7375 via
//!   [`chain::enqueue_signed_publish`]. Safe to repeat — kind:7375 is a NIP-01
//!   regular event so relays dedupe by id, and the #2965 ingest fold dedupes
//!   `TokenAdded` per token-event id (`ingest.rs`'s `is_token_live` guard), so
//!   a republish of an event that already landed folds to nothing.
//! - `minted_proofs` set, not yet signed: re-drive via
//!   [`dispatch_token_event`] — the same function `DepositResume::Minted`
//!   calls — which rebuilds and signs the kind:7375 over those same proofs
//!   (never re-mints).
//!
//! Unlike `complete`'s `DepositResume::Signed`, this republish does NOT gate on
//! `still_held`: a cold-restarted process has an empty in-memory proof
//! inventory (the crash lost it — the proofs are recovered separately by the
//! #2965 self-authored ingest path), so a `still_held` check would wrongly
//! read every restored deposit as "already spent". Republishing the cached
//! event is unconditionally safe (see above), and the settle rule
//! (`events.rs`) retires the entry once the republished event is ingested back.
//!
//! Runs off the actor thread (D8), mirroring `CashuCompleteDepositCommand::run`
//! exactly: `run` captures relays/clock/sender from `ctx` synchronously, then
//! spawns the worker.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::KIND_NIP60_TOKEN;

use crate::journal::WalletOperationId;

use super::super::chain::enqueue_signed_publish;
use super::super::state::{lock_state, CashuWalletState};
use super::dispatch_token_event;

pub(in crate::backend::cashu) struct ResumeDepositCommand {
    pub(in crate::backend::cashu) state: Arc<Mutex<CashuWalletState>>,
    pub(in crate::backend::cashu) operation_id: WalletOperationId,
    pub(in crate::backend::cashu) quote_id: String,
    pub(in crate::backend::cashu) mint: String,
    pub(in crate::backend::cashu) account_pubkey: String,
}

impl fmt::Debug for ResumeDepositCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeDepositCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

/// The chain tail a restored deposit resumes into — the analogue of
/// `complete::DepositResume`, minus the `Fresh` (never-minted) branch, which
/// this cold-restore path never re-drives (see the module docs).
enum ResumeChain {
    Signed(nmp_signer_iface::SignedEvent),
    Minted(Vec<nmp_nip60::cashu::types::Proof>),
}

impl ProtocolCommand for ResumeDepositCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            quote_id,
            mint,
            account_pubkey,
        } = *self;
        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_TOKEN);
        // D7 — the kernel owns the wall clock; re-stamp before the token event
        // is (re)built, exactly as `CashuCompleteDepositCommand::run` does.
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let resume = {
                let mut guard = lock_state(&state);
                let Some(pending) = guard.pending_deposits.get_mut(&quote_id) else {
                    // The deposit was settled/removed between restore and this
                    // command running (e.g. its kind:7375 was ingested first
                    // and the settle rule fired) — nothing left to re-drive.
                    return;
                };
                if let Some(signed) = pending.signed_token.clone() {
                    ResumeChain::Signed(signed)
                } else if let Some(proofs) = pending.minted_proofs.clone() {
                    // Stamp a fresh lease so a concurrent live retry for the
                    // same quote_id can't also enter the sign chain (same
                    // guard `DepositResume::Minted` sets).
                    pending.chain_started_at = Some(created_at);
                    ResumeChain::Minted(proofs)
                } else {
                    // Quote-created-only — never enqueued as a resume (see
                    // `wal_payload::restore_deposits`); defensive no-op.
                    return;
                }
            };
            match resume {
                ResumeChain::Signed(signed) => {
                    // No correlation id: a cold-start re-drive has no caller
                    // waiting on an action result.
                    enqueue_signed_publish(&worker_tx, &signed, relays, None);
                }
                ResumeChain::Minted(proofs) => {
                    dispatch_token_event(
                        worker_tx,
                        state,
                        operation_id,
                        quote_id,
                        mint,
                        proofs,
                        account_pubkey,
                        relays,
                        created_at,
                        None,
                    );
                }
            }
        });
        Ok(())
    }
}
