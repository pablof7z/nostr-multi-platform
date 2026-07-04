//! Send-side durable WAL (PR-3 of #2910/#2960/#2931): the `SendNutzap`
//! resume-payload write helper, the cold-restart restore/classify, and the
//! `ResumeSendCommand` that either re-drives `finish_send` from persisted
//! post-swap proofs or reconciles a reserved-but-never-swapped send against the
//! mint.
//!
//! # What #2960 is
//!
//! `send_worker::finish_send` folds the mint swap's real effect (spend the
//! selected inputs, credit the change proofs, transition the operation) only
//! inside the outgoing kind:9321's `on_signed` closure — so the swapped-but-
//! unsigned proofs (recipient's P2PK-locked outputs + sender change) live only
//! in that closure's capture until the event signs. A crash before signing
//! loses the wallet's OWN record of them (not a double-spend — the proofs are
//! valid at the mint — but the wallet forgets it holds/sent them). Persisting
//! the swap outputs here the instant `client.swap(..)` returns `Ok`, and
//! re-driving `finish_send` from them on restart, closes that window.
//!
//! # The two restart branches
//!
//! - `swapped: Some(..)` → the swap committed; re-drive `finish_send` (the same
//!   test-drivable seam `send_worker.rs` factors out) with the persisted
//!   outputs. Idempotent enough: a send that actually reached `Settled` before
//!   the crash had its WAL row+payload deleted by the terminal write-through
//!   (`state.rs`'s `wal_persist`), so only a genuinely-unfinished send is ever
//!   re-driven.
//! - `swapped: None`, non-terminal → "reserved but the swap never committed".
//!   NUT-07 check-state the reserved inputs at the mint (via the ONE call site,
//!   `check_state::query_spend_states`): all unspent → the swap never happened →
//!   fail the operation (`Failed`, terminal — which deletes the WAL row); any
//!   not-unspent → the swap may have committed and the wallet lost track →
//!   leave it `Unknown` so it stays visible in `pending_operations` as needing
//!   attention rather than silently vanishing.
//!
//! Restoring a failed send does NOT re-add the reserved inputs to the
//! inventory: a cold-started process rebuilds `state.proofs` from scratch by
//! ingesting its own live kind:7375 token events (#2965), which still carry
//! these inputs (the kind:5/kind:7375 rollover is a documented `send.rs`
//! deferral), and #2977's check-state pass then drops any the mint reports
//! spent. Re-adding here would double-count against that rebuild. This mirrors
//! `deposit/resume.rs`'s own divergence from the in-process retry path (it
//! skips `still_held` for the same empty-cold-inventory reason).

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::cashu::types::ProofSpendState;

use crate::journal::{WalletOperationId, WalletOperationKind, WalletOperationState, WalletWalStore};

use super::check_state::query_spend_states;
use super::send_worker::{finish_send, FinishSendArgs};
use super::state::{lock_state, CashuWalletState, StoredProof};
use super::wal_payload::{CashuWalPayload, StoredProofRecord, SwappedSend};

/// Write the current send resume payload for `operation_id` through to the
/// durable WAL. A no-op when no WAL/account is configured (in-memory-only
/// parity). Failures are swallowed — the payload is a durability shadow and
/// must never fail the in-memory mutation that already succeeded (D6), exactly
/// like `state.rs`'s saga write-through and `wal_payload.rs`'s deposit variant.
///
/// Called at the two send write points: at reservation (`send.rs`, `swapped:
/// None`, same lock scope as the `remove_proofs` reservation) and after the
/// mint swap returns `Ok` (`send_worker.rs`, `swapped: Some`, before
/// `finish_send`).
#[allow(clippy::too_many_arguments)]
pub(super) fn persist_send_payload(
    state: &CashuWalletState,
    operation_id: &WalletOperationId,
    mint: &str,
    recipient_pubkey: &str,
    recipient_cashu_pubkey: &str,
    target_event_id: Option<&str>,
    relays: &[String],
    selected: &[StoredProof],
    swapped: Option<SwappedSend>,
) {
    let (Some(store), Some(account)) = (state.wal_store.as_ref(), state.wal_account.as_ref()) else {
        return;
    };
    let payload = CashuWalPayload::Send {
        mint: mint.to_string(),
        recipient_pubkey: recipient_pubkey.to_string(),
        recipient_cashu_pubkey: recipient_cashu_pubkey.to_string(),
        target_event_id: target_event_id.map(str::to_string),
        relays: relays.to_vec(),
        selected: selected.iter().map(StoredProofRecord::from).collect(),
        swapped,
    };
    if let Some(bytes) = payload.encode() {
        let _ = store.upsert_payload(account, operation_id, &bytes);
    }
}

/// A restored send needing either a `finish_send` re-drive (`swapped: Some`) or
/// a mint check-state reconcile (`swapped: None`). `restore_sends` decodes
/// these from the WAL; `lifecycle.rs` wraps each in a [`ResumeSendCommand`] the
/// caller enqueues onto the actor (the re-drive/reconcile mint HTTP must run
/// off the actor thread, D8 — same seam `ResumeDepositCommand` uses).
pub(super) struct ResumeSend {
    pub(super) operation_id: WalletOperationId,
    pub(super) mint: String,
    pub(super) recipient_pubkey: String,
    pub(super) target_event_id: Option<String>,
    pub(super) relays: Vec<String>,
    pub(super) selected: Vec<StoredProof>,
    pub(super) swapped: Option<SwappedSend>,
}

/// Read back every non-terminal `SendNutzap` operation's payload from the WAL
/// for `account`. Called from `restore_from_wal` AFTER `restore_into_journal`
/// has rehydrated the saga journal and deleted terminal rows, so every send op
/// seen here is genuinely in-flight and still needs resolving.
pub(super) fn restore_sends(store: &dyn WalletWalStore, account: &str) -> Vec<ResumeSend> {
    let mut resumes = Vec::new();
    let Ok(operations) = store.load_operations(account) else {
        return resumes;
    };
    for op in operations {
        if op.kind != WalletOperationKind::SendNutzap || op.state.is_terminal() {
            continue;
        }
        let Ok(Some(bytes)) = store.load_payload(account, &op.id) else {
            continue;
        };
        let Some(CashuWalPayload::Send {
            mint,
            recipient_pubkey,
            // Persisted for forward-compat (a future re-swap path would need
            // it), but the restart branches here never re-issue the swap, so
            // it is not threaded into `ResumeSend`.
            recipient_cashu_pubkey: _,
            target_event_id,
            relays,
            selected,
            swapped,
        }) = CashuWalPayload::decode(&bytes)
        else {
            continue;
        };
        resumes.push(ResumeSend {
            operation_id: op.id,
            mint,
            recipient_pubkey,
            target_event_id,
            relays,
            selected: selected.into_iter().map(StoredProofRecord::into_stored).collect(),
            swapped,
        });
    }
    resumes
}

pub(super) struct ResumeSendCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) account_pubkey: String,
    pub(super) resume: ResumeSend,
}

impl fmt::Debug for ResumeSendCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeSendCommand")
            .field("operation_id", &self.resume.operation_id.as_str())
            .field("swapped", &self.resume.swapped.is_some())
            .finish()
    }
}

impl ProtocolCommand for ResumeSendCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            account_pubkey,
            resume,
        } = *self;
        // D7 — re-stamp the wall clock before the kind:9321 is (re)built, the
        // same way `ResumeDepositCommand::run` does.
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let ResumeSend {
                operation_id,
                mint,
                recipient_pubkey,
                target_event_id,
                relays,
                selected,
                swapped,
            } = resume;
            match swapped {
                Some(SwappedSend {
                    new_proofs,
                    nutzap_count,
                }) => {
                    // The swap committed pre-crash; finish the exact chain the
                    // live worker would have (build/sign/publish kind:9321,
                    // fold on sign). No correlation id — a cold re-drive has no
                    // caller waiting on an action result.
                    finish_send(FinishSendArgs {
                        worker_tx,
                        state,
                        operation_id,
                        account_pubkey,
                        recipient_pubkey,
                        mint,
                        selected,
                        new_proofs,
                        nutzap_count,
                        target_event_id,
                        relays,
                        created_at,
                        correlation_id: None,
                    });
                }
                None => reconcile_reserved_send(&state, &operation_id, &mint, &selected),
            }
        });
        Ok(())
    }
}

/// The `swapped: None` restart branch: NUT-07 check-state the reserved inputs
/// and either fail the send (swap never committed) or leave it `Unknown` for
/// attention (swap may have committed and the wallet lost track). A mint HTTP
/// failure leaves the operation exactly as-is — the same fail-safe every
/// `query_spend_states` caller shares — so the next restart tries again.
fn reconcile_reserved_send(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    mint: &str,
    selected: &[StoredProof],
) {
    let secrets: Vec<String> = selected.iter().map(|s| s.proof.secret.clone()).collect();
    let Some(states) = query_spend_states(mint, &secrets) else {
        return;
    };
    let all_unspent = states.iter().all(|st| *st == ProofSpendState::Unspent);
    let mut s = lock_state(state);
    if all_unspent {
        // The swap never committed — the inputs are still good and will come
        // back through this account's own kind:7375 ingest (#2965). Fail the
        // operation (terminal → the WAL row+payload are deleted by the terminal
        // write-through). Do NOT re-add the inputs here (see module docs).
        let _ = s.transition(operation_id, WalletOperationState::Failed);
    } else {
        // At least one input is spent/pending — the swap may have committed and
        // the swap outputs were lost pre-crash. Never fail (that would let the
        // send look cleanly abandoned); leave it `Unknown` so it stays in
        // `pending_operations` as needing attention.
        let _ = s.transition(operation_id, WalletOperationState::Unknown);
    }
}

/// The journal state a restored send's operation must be in for
/// [`reconcile_reserved_send`] to move it — a defensive helper the send-side
/// cold-restore tests assert against. `MintPending` is the state the live
/// reservation leaves it in before the swap; a crash mid-swap may also leave
/// it `Unknown` (the worker's `mark_uncertain`). Both are non-terminal and both
/// can transition to `Failed`/`Unknown`, so the reconcile is valid from either.
#[cfg(test)]
pub(super) fn is_reconcilable_send_state(state: WalletOperationState) -> bool {
    matches!(state, WalletOperationState::MintPending | WalletOperationState::Unknown)
}
