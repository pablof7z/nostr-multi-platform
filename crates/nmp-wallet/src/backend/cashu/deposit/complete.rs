//! [`CashuCompleteDepositCommand`] — phase two of the two-phase Cashu deposit
//! flow: the value-moving NUT-04 mint + the #2910/#2923 money-safety
//! resume/lease/fencing state machine. See [`super`]'s module docs for the
//! flow overview and the documented "fenced-out attempt still publishes"
//! limitation; see [`PendingDeposit`](super::super::state::PendingDeposit)'s
//! doc comment for the `minted_proofs`/`signed_token`/`chain_started_at`
//! field semantics this file drives.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{
    build_record_action_success, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::{MintQuoteState, Proof};
use nmp_nip60::cashu::MintClient;
use nmp_nip60::KIND_NIP60_TOKEN;
use nmp_signer_iface::SignedEvent;

use crate::journal::{WalletOperationId, WalletOperationState};

use super::super::chain;
use super::super::state::{lock_state, CashuWalletState};
use super::super::ui_codes;
use super::{dispatch_token_event, fail};

pub(in crate::backend::cashu) struct CashuCompleteDepositCommand {
    pub(in crate::backend::cashu) state: Arc<Mutex<CashuWalletState>>,
    pub(in crate::backend::cashu) operation_id: WalletOperationId,
    pub(in crate::backend::cashu) quote_id: String,
    pub(in crate::backend::cashu) mint: String,
    pub(in crate::backend::cashu) amount_sats: u64,
    pub(in crate::backend::cashu) account_pubkey: String,
    pub(in crate::backend::cashu) correlation_id: Option<String>,
}

impl fmt::Debug for CashuCompleteDepositCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CashuCompleteDepositCommand")
            .field("operation_id", &self.operation_id.as_str())
            .finish()
    }
}

/// Where a `CompleteDepositCashu` attempt for a given `quote_id` picks back
/// up — see the doc comment on `CashuCompleteDepositCommand::run`'s `resume`
/// binding for the ordering/safety rationale of each variant.
enum DepositResume {
    Signed(SignedEvent),
    Minted(Vec<Proof>),
    Fresh,
}

/// How long a `chain_started_at` lease (see `PendingDeposit`'s doc comment)
/// blocks a concurrent retry before the previous attempt is presumed
/// abandoned and a new one is allowed to take over. Generous enough to cover
/// a real mint HTTP round-trip plus the encrypt/sign actor round-trips
/// (seconds, not the tens-of-seconds a slow mint might take) without making
/// a genuinely stuck deposit wait long for a legitimate retry.
const DEPOSIT_CHAIN_LEASE_SECS: u64 = 60;

impl ProtocolCommand for CashuCompleteDepositCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            quote_id,
            mint,
            amount_sats,
            account_pubkey,
            correlation_id,
        } = *self;
        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_TOKEN);
        // D7 — the kernel owns the wall clock; re-stamp before the token
        // event is built (see `chain.rs`'s `launch_self_encrypted_publish`).
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            // Three resume points, checked in the order of "how much
            // value-moving/signing work already happened" — never redo a
            // step that genuinely already ran:
            //
            // - `signed_token` set: the kind:7375 event was already SIGNED
            //   by a prior attempt (see `PendingDeposit::signed_token`'s doc
            //   comment — this is the #2910/#2923 fix: signing can succeed
            //   while the publish right after it fails). Never re-sign —
            //   only re-publish the identical cached event.
            // - Neither `signed_token` set NOR a chain currently in flight
            //   (`chain_started_at`, a self-healing lease — see
            //   `PendingDeposit`'s doc comment): resume from `minted_proofs`
            //   if a prior attempt already got real proofs back but failed
            //   before reaching signing (a transient port error, a
            //   dead-but-still-alive actor inbox), or mint fresh if not. The
            //   lease is what stops a CONCURRENT retry for the SAME
            //   `quote_id` from also entering here and racing a second sign
            //   chain over the same proofs — that would sign two
            //   differently-id'd token events for one real deposit and
            //   double-fold the ledger (no proof-identity dedup, only
            //   token-event-id dedup).
            // - A chain is already in flight and hasn't been signed yet:
            //   fail closed, retryable — wait for the lease to clear (either
            //   the in-flight attempt finishes, or the lease expires and
            //   presumes it abandoned).
            //
            // Both `minted_proofs`/`signed_token` are in-memory only (see
            // their doc comments in `state.rs`): a hard crash before either
            // is set still loses the deposit — that durable gap is tracked
            // as issue #2910, not closed here.
            let resume = {
                let mut guard = lock_state(&state);
                match guard.pending_deposits.get_mut(&quote_id) {
                    Some(pending) => {
                        if let Some(signed) = pending.signed_token.clone() {
                            DepositResume::Signed(signed)
                        } else if let Some(started_at) = pending.chain_started_at {
                            if created_at.saturating_sub(started_at) < DEPOSIT_CHAIN_LEASE_SECS {
                                fail(
                                    &worker_tx,
                                    correlation_id,
                                    ui_codes::DEPOSIT_IN_PROGRESS,
                                    "a completion attempt for this deposit is already in progress — retry shortly".to_string(),
                                );
                                return;
                            }
                            // Lease expired — the previous attempt is presumed
                            // abandoned; take over.
                            pending.chain_started_at = Some(created_at);
                            match pending.minted_proofs.clone() {
                                Some(proofs) => DepositResume::Minted(proofs),
                                None => DepositResume::Fresh,
                            }
                        } else {
                            pending.chain_started_at = Some(created_at);
                            match pending.minted_proofs.clone() {
                                Some(proofs) => DepositResume::Minted(proofs),
                                None => DepositResume::Fresh,
                            }
                        }
                    }
                    None => {
                        fail(
                            &worker_tx,
                            correlation_id,
                            ui_codes::UNKNOWN_QUOTE,
                            "no pending deposit for this quote".to_string(),
                        );
                        return;
                    }
                }
            };

            let proofs = match resume {
                DepositResume::Signed(signed) => {
                    // These proofs may since have been spent (e.g. sent as a
                    // nutzap) and their token event superseded by a newer
                    // one — republishing a fully-spent, already-superseded
                    // event would just resurrect stale content with nothing
                    // behind it. Require the FULL original proof set to
                    // still be held (not just one of them — a partial spend
                    // means the token event's remaining content is already
                    // stale too) before republishing.
                    let still_held = {
                        let guard = lock_state(&state);
                        guard
                            .pending_deposits
                            .get(&quote_id)
                            .and_then(|p| p.minted_proofs.as_ref())
                            .is_some_and(|original| {
                                let held: std::collections::HashSet<&str> = guard
                                    .proofs
                                    .iter()
                                    .filter(|p| p.token_event.as_deref() == Some(signed.id.as_str()))
                                    .map(|p| p.proof.c.as_str())
                                    .collect();
                                original.iter().all(|p| held.contains(p.c.as_str()))
                            })
                    };
                    if still_held {
                        chain::enqueue_signed_publish(&worker_tx, &signed, relays, correlation_id);
                    } else if let Some(id) = correlation_id {
                        let result_json =
                            serde_json::json!({ "already_settled_and_spent": true }).to_string();
                        let _ = worker_tx.send(build_record_action_success(id, Some(result_json)));
                    }
                    return;
                }
                DepositResume::Minted(proofs) => proofs,
                DepositResume::Fresh => {
                    let client = MintClient::new(&mint);
                    let status = match client.get_mint_quote_status(&quote_id) {
                        Ok(s) => s,
                        Err(e) => {
                            clear_chain_lease(&state, &quote_id, created_at);
                            fail(
                                &worker_tx,
                                correlation_id,
                                ui_codes::MINT_QUOTE_FAILED,
                                format!("mint quote status check failed: {e}"),
                            );
                            return;
                        }
                    };
                    if status.state != MintQuoteState::Paid {
                        // Retryable, not a hard failure — the caller pays the
                        // invoice (testnut auto-settles almost immediately) and
                        // retries `CompleteDepositCashu`. The operation stays at
                        // `MintSettled`; no journal transition here.
                        clear_chain_lease(&state, &quote_id, created_at);
                        fail(
                            &worker_tx,
                            correlation_id,
                            ui_codes::QUOTE_NOT_PAID,
                            "mint quote not yet paid — pay the invoice and retry".to_string(),
                        );
                        return;
                    }
                    let keyset = match client.get_sat_keyset() {
                        Ok(k) => k,
                        Err(e) => {
                            clear_chain_lease(&state, &quote_id, created_at);
                            mark_operation_uncertain(
                                &state,
                                &operation_id,
                                &worker_tx,
                                correlation_id,
                                format!("mint keyset fetch failed: {e}"),
                            );
                            return;
                        }
                    };
                    // THE value-moving call. The operation's `MintSettled`
                    // pre-record (written back durably by
                    // `CashuDepositQuoteCommand`, before any HTTP happened)
                    // already satisfies the at-most-once guarantee —
                    // deposits consume zero local proofs (see
                    // `WalletOperationKind::requires_consumed_inputs_before_mint_request`),
                    // so there is nothing further to pre-record before this
                    // call.
                    let proofs = match client.mint_tokens(&quote_id, amount_sats, &keyset) {
                        Ok(p) => p,
                        Err(e) => {
                            clear_chain_lease(&state, &quote_id, created_at);
                            mark_operation_uncertain(
                                &state,
                                &operation_id,
                                &worker_tx,
                                correlation_id,
                                format!("mint-tokens request failed: {e}"),
                            );
                            return;
                        }
                    };
                    // Persist BEFORE handing off to the encrypt/sign/publish
                    // chain — from here on, a chain failure must resume from
                    // these proofs, never re-mint (see `PendingDeposit::minted_proofs`).
                    // Write-if-absent, NOT fenced on `chain_started_at`
                    // (#2946): unlike `on_signed`'s fold, there is no
                    // clobber to guard against here — NUT-04 single-issue
                    // means at most one attempt for a given `quote_id` ever
                    // reaches this point with real proofs (every other
                    // attempt's own `mint_tokens` call fails closed with
                    // "already issued" and returns before reaching here), so
                    // a `chain_started_at` mismatch just means a NEWER
                    // attempt took over the lease while THIS one was still
                    // waiting on a slow mint response. Fencing the write on
                    // that mismatch silently drops the only copy of real,
                    // already-minted proofs — stranding real sats (the bug
                    // #2946 fixed). `minted_proofs.is_none()` is the correct
                    // guard: never overwrite a set of proofs a prior attempt
                    // already recorded.
                    let mut guard = lock_state(&state);
                    if let Some(pending) = guard.pending_deposits.get_mut(&quote_id) {
                        if pending.minted_proofs.is_none() {
                            pending.minted_proofs = Some(proofs.clone());
                        }
                    }
                    drop(guard);
                    proofs
                }
            };
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
                correlation_id,
            );
        });
        Ok(())
    }
}

/// Release a `chain_started_at` lease (see `PendingDeposit`'s doc comment)
/// after a synchronous, pre-chain failure — one that returns from within
/// this SAME worker thread, before any encrypt/sign continuation was ever
/// enqueued. Without this, a caller that pays the invoice a few seconds
/// after a `QUOTE_NOT_PAID` response (or retries right after a
/// `MINT_QUOTE_FAILED`/`MINT_TOKENS_FAILED`) would be told
/// `DEPOSIT_IN_PROGRESS` for up to `DEPOSIT_CHAIN_LEASE_SECS` even though
/// nothing is actually in flight. Async chain failures (`chain.rs`'s
/// encrypt/sign errors) have no such hook back into this state — those
/// self-heal only once the lease naturally expires (see
/// `PendingDeposit::chain_started_at`'s doc comment).
///
/// `lease` is a FENCING token, not just a value to blank out: only clears
/// the lease if it still equals `lease` (the `created_at` THIS attempt
/// stamped it with when it took over). Without this compare-and-clear, a
/// slow-but-not-actually-dead attempt A that fails after its lease already
/// expired and was taken over by a newer attempt B would blank out B's
/// (different, newer) lease instead of its own — reopening the door for a
/// THIRD attempt C to race B. `created_at` is safe to use as the fencing
/// token because a takeover only ever happens once the elapsed time since
/// the previous `created_at` exceeds `DEPOSIT_CHAIN_LEASE_SECS`, so two
/// attempts that both believe themselves "current" always carry distinct
/// `created_at` values.
fn clear_chain_lease(state: &Mutex<CashuWalletState>, quote_id: &str, lease: u64) {
    let mut guard = lock_state(state);
    if let Some(pending) = guard.pending_deposits.get_mut(quote_id) {
        if pending.chain_started_at == Some(lease) {
            pending.chain_started_at = None;
        }
    }
}

/// An HTTP failure talking to the mint mid-mint-request is genuinely
/// ambiguous — the mint may have issued the tokens and the response was
/// merely lost in transit, or the request may never have been accepted at
/// all. `Unknown` (not `Failed`, which is terminal and can never transition
/// again) is the state the saga's own transition table defines for exactly
/// this "operation crashed, resolution still pending" case: a retry that
/// recovers (see `PendingDeposit::minted_proofs`) can still reach
/// `PublishPending`/`Settled` from `Unknown`, whereas a `Failed` operation
/// would be stuck forever even after the deposit actually completed.
fn mark_operation_uncertain(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    reason: String,
) {
    {
        let mut guard = lock_state(state);
        let _ = guard.transition(operation_id, WalletOperationState::Unknown);
    }
    fail(
        worker_tx,
        correlation_id,
        ui_codes::MINT_TOKENS_FAILED,
        reason,
    );
}
