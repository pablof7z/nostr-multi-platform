//! Redeem-side durable WAL (PR-3 of #2910/#2960/#2931): the `RedeemNutzap`
//! resume-payload write helper, the cold-restart restore/classify, and the
//! `ResumeRedeemCommand` that either re-drives `finish_redeem` from persisted
//! post-swap proofs or reconciles a stuck-`Unknown` redeem against the mint.
//!
//! # What #2931 is
//!
//! A redeem that lands in `WalletOperationState::Unknown` after a transient
//! mint failure (an ambiguous HTTP failure during `client.get_sat_keyset()`/
//! `client.swap()` — `redeem_worker.rs`'s `mark_uncertain`) is never
//! reconciled or retried without a process restart: nothing drives the
//! `Unknown -> {Failed, ...}` transition the state machine already allows. This
//! module is what drives it, on restart, off the actor thread (D8).
//!
//! # The two restart branches
//!
//! - `fresh_proofs: Some(..)` → the unlinking swap committed; re-drive
//!   `finish_redeem` (the same test-drivable seam `redeem_worker.rs` factors
//!   out) with the persisted fresh proofs — publish kind:7375 + kind:7376 and
//!   fold on the final sign, exactly as the live worker would. A redeem that
//!   actually reached `Settled` before the crash had its WAL row+payload
//!   deleted by the terminal write-through, so only a genuinely-unfinished
//!   redeem is re-driven.
//! - `fresh_proofs: None`, non-terminal (`Unknown`/`MintPending`) → the swap
//!   never returned `Ok`. NUT-07 check-state the nutzap's OWN input-proof
//!   secrets at the mint (via the ONE call site,
//!   `check_state::query_spend_states`):
//!   - all unspent → the swap never committed → **the #2931 fix**: delete the
//!     saga row from BOTH the durable WAL and the in-memory journal (mirroring
//!     `restore_into_journal`'s terminal-deletion), so the
//!     `DuplicateOperation` guard on `begin_operation(redeem-{event_id})` no
//!     longer blocks a re-dispatch when the kind:9321 is naturally re-observed
//!     later. This re-proves the pre-durable-journal self-heal through a
//!     positive mint verdict instead of "the journal evaporated".
//!   - any not-unspent → the swap DID commit; the fresh proofs are real but the
//!     wallet lost track of them pre-crash → leave the operation `Unknown` so it
//!     stays visible in `pending_operations` as needing attention rather than
//!     silently dropping value.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_nip60::cashu::types::{Proof, ProofSpendState};
use nmp_nip60::nutzap::ReceivedNutZap;

use crate::journal::{
    WalletEventId, WalletOperationId, WalletOperationKind, WalletOperationState, WalletWalStore,
};

use super::check_state::query_spend_states;
use super::redeem_worker::{finish_redeem, FinishRedeemArgs};
use super::state::{lock_state, CashuWalletState};
use super::wal_payload::{CashuWalPayload, NutzapRecord};

/// Write the current redeem resume payload for `operation_id` through to the
/// durable WAL. A no-op when no WAL/account is configured. Failures are
/// swallowed (D6 — durability shadow), exactly like the send/deposit variants.
///
/// Called at the two redeem write points: pre-swap (`redeem.rs`, `fresh_proofs:
/// None`, the instant the nutzap/mint/relays are all known — mirroring the
/// deposit "record as soon as you have enough to resume" pattern) and after the
/// unlinking swap returns `Ok` (`redeem_worker.rs`, `fresh_proofs: Some`,
/// before `finish_redeem`).
pub(super) fn persist_redeem_payload(
    state: &CashuWalletState,
    operation_id: &WalletOperationId,
    nutzap: &ReceivedNutZap,
    nutzap_wallet_event: &WalletEventId,
    relays: &[String],
    fresh_proofs: Option<Vec<Proof>>,
) {
    let (Some(store), Some(account)) = (state.wal_store.as_ref(), state.wal_account.as_ref()) else {
        return;
    };
    let payload = CashuWalPayload::Redeem {
        nutzap: NutzapRecord::from(nutzap),
        nutzap_wallet_event: nutzap_wallet_event.as_str().to_string(),
        mint: nutzap.mint_url.clone(),
        relays: relays.to_vec(),
        fresh_proofs,
    };
    if let Some(bytes) = payload.encode() {
        let _ = store.upsert_payload(account, operation_id, &bytes);
    }
}

/// A restored redeem needing either a `finish_redeem` re-drive (`fresh_proofs:
/// Some`) or a mint check-state reconcile (`fresh_proofs: None`). Carries the
/// rebuilt `nutzap` (from [`NutzapRecord::into_received`]); an entry whose blob
/// is corrupt/foreign or whose nutzap hex won't parse is dropped on decode
/// (the WAL's corrupt-skip discipline).
pub(super) struct ResumeRedeem {
    pub(super) operation_id: WalletOperationId,
    pub(super) nutzap: ReceivedNutZap,
    pub(super) nutzap_wallet_event: WalletEventId,
    pub(super) mint: String,
    pub(super) relays: Vec<String>,
    pub(super) fresh_proofs: Option<Vec<Proof>>,
}

/// Read back every non-terminal `RedeemNutzap` operation's payload from the WAL
/// for `account`. Called from `restore_from_wal` AFTER `restore_into_journal`
/// has rehydrated the journal and deleted terminal rows.
pub(super) fn restore_redeems(store: &dyn WalletWalStore, account: &str) -> Vec<ResumeRedeem> {
    let mut resumes = Vec::new();
    let Ok(operations) = store.load_operations(account) else {
        return resumes;
    };
    for op in operations {
        if op.kind != WalletOperationKind::RedeemNutzap || op.state.is_terminal() {
            continue;
        }
        let Ok(Some(bytes)) = store.load_payload(account, &op.id) else {
            continue;
        };
        let Some(CashuWalPayload::Redeem {
            nutzap,
            nutzap_wallet_event,
            mint,
            relays,
            fresh_proofs,
        }) = CashuWalPayload::decode(&bytes)
        else {
            continue;
        };
        let Some(nutzap) = nutzap.into_received() else {
            continue;
        };
        resumes.push(ResumeRedeem {
            operation_id: op.id,
            nutzap,
            nutzap_wallet_event: WalletEventId::new(nutzap_wallet_event),
            mint,
            relays,
            fresh_proofs,
        });
    }
    resumes
}

pub(super) struct ResumeRedeemCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) account_pubkey: String,
    pub(super) resume: ResumeRedeem,
}

impl fmt::Debug for ResumeRedeemCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeRedeemCommand")
            .field("operation_id", &self.resume.operation_id.as_str())
            .field("fresh_proofs", &self.resume.fresh_proofs.is_some())
            .finish()
    }
}

impl ProtocolCommand for ResumeRedeemCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            account_pubkey,
            resume,
        } = *self;
        // D7 — re-stamp the wall clock before kind:7375/7376 are (re)built.
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            let ResumeRedeem {
                operation_id,
                nutzap,
                nutzap_wallet_event,
                mint,
                relays,
                fresh_proofs,
            } = resume;
            match fresh_proofs {
                Some(fresh_proofs) => {
                    // The unlinking swap committed pre-crash; finish the exact
                    // chain the live worker would (publish kind:7375 then
                    // kind:7376, fold on the final sign). No correlation id — a
                    // cold re-drive has no caller waiting on an action result.
                    finish_redeem(FinishRedeemArgs {
                        worker_tx,
                        state,
                        operation_id,
                        account_pubkey,
                        nutzap,
                        nutzap_wallet_event,
                        fresh_proofs,
                        relays,
                        created_at,
                        correlation_id: None,
                    });
                }
                None => reconcile_unknown_redeem(&state, &operation_id, &mint, &nutzap),
            }
        });
        Ok(())
    }
}

/// The `fresh_proofs: None` restart branch (the actual #2931 fix): NUT-07
/// check-state the nutzap's input-proof secrets and either delete the saga row
/// entirely (swap never committed → re-dispatchable) or leave it `Unknown` for
/// attention (swap committed, value lost track of). A mint HTTP failure leaves
/// the operation exactly as-is (the shared `query_spend_states` fail-safe), so
/// the next restart tries again.
fn reconcile_unknown_redeem(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    mint: &str,
    nutzap: &ReceivedNutZap,
) {
    let secrets: Vec<String> = nutzap.proofs.iter().map(|p| p.secret.clone()).collect();
    let Some(states) = query_spend_states(mint, &secrets) else {
        return;
    };
    let all_unspent = states.iter().all(|st| *st == ProofSpendState::Unspent);
    let mut s = lock_state(state);
    if all_unspent {
        // The swap never committed — delete the row from the durable WAL AND
        // the in-memory journal so a re-observed kind:9321 can `begin_operation`
        // cleanly (the #2931 re-dispatch unblock). Deleting via the store
        // directly (not a `Failed` transition) is deliberate: a `Failed` row
        // would still live in the in-memory journal and keep blocking the
        // `DuplicateOperation` guard — the very hazard this fix removes.
        if let (Some(store), Some(account)) =
            (s.wal_store.clone(), s.wal_account.clone())
        {
            let _ = store.delete_operation(&account, operation_id);
            let _ = store.delete_payload(&account, operation_id);
        }
        s.journal.remove(operation_id);
    } else {
        // At least one input is spent/pending — the swap committed and the
        // fresh proofs were lost pre-crash. Leave it `Unknown` so it surfaces in
        // `pending_operations` as needing attention; never delete/drop it.
        let _ = s.transition(operation_id, WalletOperationState::Unknown);
    }
}
