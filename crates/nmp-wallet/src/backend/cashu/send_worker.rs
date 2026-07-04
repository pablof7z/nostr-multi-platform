//! `SendNutzap`'s worker-thread half — split out of `send.rs` (AGENTS.md
//! file-size discipline). See that file's module docs for the recipient
//! resolution and proof-reservation this worker's caller
//! (`SendNutzapCommand::run`) already performed before spawning it.
//!
//! # Definite vs. ambiguous failure
//!
//! `SendNutzapCommand::run` reserves `selected` (removes it from the proof
//! inventory) BEFORE spawning this worker, to close the double-tap race
//! described there. That means every failure path here must decide: was the
//! reservation ever actually consumed by the mint?
//!
//! - Before `client.swap(..)` is called (keyset fetch, fee check): NO —
//!   [`restore_and_fail`] undoes the reservation and fails `Failed`.
//! - At or after `client.swap(..)`: MAYBE — the mint may have committed the
//!   swap even if the response was lost in transit, so [`mark_uncertain`]
//!   leaves the reservation in place and fails `Unknown` (never restore an
//!   input that might already be spent).
//!
//! # Fold-after-sign ordering (#2934 money-safety fix)
//!
//! Once `client.swap(..)` itself returns `Ok`, the mint HAS committed —
//! `finish_send` is everything after that. Its ledger fold (marking
//! `selected` spent, crediting the change proofs, the `MintSettled`
//! transition) only runs inside the outgoing kind:9321's `on_signed`
//! closure, never before — mirrors `redeem_worker.rs`'s `finish_redeem`
//! (fold only in the LAST `on_signed` of its own chain), and for the
//! identical reason: `chain.rs::sign_and_publish` returns on a sign `Err`
//! WITHOUT calling `on_signed`, so folding before that point would debit the
//! sender's reported balance for a nutzap that was never actually signed or
//! published — the recipient gets nothing while the sender's balance already
//! reflects the send. If sign fails, none of that fold runs: the operation
//! simply never advances past `MintPending`, and the ledger keeps showing
//! the pre-send balance. This has the same accepted #2910-class gap
//! `finish_redeem` documents: the swap's real outputs (`nutzap_proofs`,
//! `change_proofs`) live only in this closure's capture until folded, so a
//! sign failure (or a crash) still loses track of them for THIS attempt —
//! not closed by this fix, tracked as #2910 — but it can never double-debit
//! or silently strand a debit with no matching effect, which is what #2934
//! closes.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_record_action_failure;
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::cashu::{split_amount, MintClient};
use nmp_nip60::kinds::KIND_NIP61_NUTZAP;
use nmp_nip60::nutzap::{nutzap_event_tags, p2pk_secret, NutZapProof};

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, Provenance, WalletEventId, WalletFact, WalletOperationId,
    WalletOperationState, WalletUnit,
};

use super::chain::launch_plain_publish;
use super::state::{canonicalize_mint_url, lock_state, CashuWalletState, StoredProof};
use super::ui_codes;

pub(super) struct SendWorkerArgs {
    pub(super) worker_tx: CommandSender,
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) recipient_pubkey: String,
    pub(super) mint: String,
    pub(super) selected: Vec<StoredProof>,
    pub(super) selected_total: u64,
    pub(super) amount_sats: u64,
    pub(super) recipient_cashu_pubkey: String,
    pub(super) target_event_id: Option<String>,
    pub(super) relays: Vec<String>,
    pub(super) created_at: u64,
    pub(super) correlation_id: Option<String>,
}

pub(super) fn run_send_worker(args: SendWorkerArgs) {
    let SendWorkerArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        recipient_pubkey,
        mint,
        selected,
        selected_total,
        amount_sats,
        recipient_cashu_pubkey,
        target_event_id,
        relays,
        created_at,
        correlation_id,
    } = args;

    let client = MintClient::new(&mint);
    let keyset = match client.get_sat_keyset() {
        Ok(k) => k,
        Err(e) => {
            // Fetching the keyset never touches `selected` — no swap was
            // attempted, so this is a definite (not ambiguous) failure:
            // restore the reserved proofs rather than stranding them.
            restore_and_fail(
                &state,
                &operation_id,
                &selected,
                &mint,
                &worker_tx,
                correlation_id,
                ui_codes::SWAP_FAILED,
                format!("mint keyset fetch failed: {e}"),
            );
            return;
        }
    };

    let output_amounts = split_amount(amount_sats);
    let p2pk_secrets: Vec<String> = output_amounts
        .iter()
        .map(|_| p2pk_secret(&recipient_cashu_pubkey))
        .collect();
    let fee = MintClient::compute_fee(selected.len() as u64, keyset.input_fee_ppk);
    let gross_change = selected_total.saturating_sub(amount_sats);
    if gross_change < fee {
        // The fee estimate wasn't knowable before the keyset round-trip —
        // this operation consumed no mint call yet, so `Failed` (not
        // `Unknown`) is honest: nothing external happened. Restore the
        // reserved proofs for the same reason.
        restore_and_fail(
            &state,
            &operation_id,
            &selected,
            &mint,
            &worker_tx,
            correlation_id,
            ui_codes::INSUFFICIENT_BALANCE,
            "selected proofs do not cover the mint fee".to_string(),
        );
        return;
    }
    let change_amount = gross_change - fee;
    let mut all_output_amounts = output_amounts.clone();
    let mut all_secrets: Vec<String> = p2pk_secrets;
    if change_amount > 0 {
        let change_denoms = split_amount(change_amount);
        all_output_amounts.extend_from_slice(&change_denoms);
        for _ in &change_denoms {
            all_secrets.push(nmp_nip60::cashu::random_secret_hex());
        }
    }

    let input_proofs: Vec<Proof> = selected.iter().map(|s| s.proof.clone()).collect();
    let new_proofs = match client.swap(input_proofs, all_output_amounts, Some(all_secrets), &keyset)
    {
        Ok(p) => p,
        Err(e) => {
            mark_uncertain(&state, &operation_id);
            fail_worker(
                &worker_tx,
                correlation_id,
                ui_codes::SWAP_FAILED,
                format!("swap failed: {e}"),
            );
            return;
        }
    };

    // #2960 — the swap has committed: persist the outputs (the recipient's
    // P2PK-locked proofs + the sender's change) BEFORE `finish_send` runs, so a
    // crash in the swapped-but-kind:9321-unsigned window can re-drive
    // `finish_send` from these exact proofs on restart (`wal_send.rs`) instead
    // of losing the wallet's own record of them. `nutzap_count`
    // (`output_amounts.len()`) is where `finish_send` splits `new_proofs` into
    // recipient outputs vs. change.
    {
        let s = lock_state(&state);
        super::wal_send::persist_send_payload(
            &s,
            &operation_id,
            &mint,
            &recipient_pubkey,
            &recipient_cashu_pubkey,
            target_event_id.as_deref(),
            &relays,
            &selected,
            Some(super::wal_payload::SwappedSend {
                new_proofs: new_proofs.clone(),
                nutzap_count: output_amounts.len(),
            }),
        );
    }

    finish_send(FinishSendArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        recipient_pubkey,
        mint,
        selected,
        new_proofs,
        nutzap_count: output_amounts.len(),
        target_event_id,
        relays,
        created_at,
        correlation_id,
    });
}

pub(super) struct FinishSendArgs {
    pub(super) worker_tx: CommandSender,
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) recipient_pubkey: String,
    pub(super) mint: String,
    pub(super) selected: Vec<StoredProof>,
    /// The mint's `swap` response — the first `nutzap_count` entries are the
    /// recipient's P2PK-locked outputs, the rest are change.
    pub(super) new_proofs: Vec<Proof>,
    pub(super) nutzap_count: usize,
    pub(super) target_event_id: Option<String>,
    pub(super) relays: Vec<String>,
    pub(super) created_at: u64,
    pub(super) correlation_id: Option<String>,
}

/// Everything after the mint has already committed the swap: build
/// kind:9321, sign, publish, and — only once that sign actually succeeds —
/// fold local state. Split out of `run_send_worker` so tests can drive this
/// directly with synthetic post-swap proofs — the DHKE blind/unblind math
/// itself is `nmp-nip60`'s own already-tested surface (mirrors
/// `deposit.rs`'s `dispatch_token_event` split for the identical reason).
///
/// See this file's module docs ("Fold-after-sign ordering") for why the
/// fold moved into `on_signed` instead of running here up front.
pub(super) fn finish_send(args: FinishSendArgs) {
    let FinishSendArgs {
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
        correlation_id,
    } = args;

    let nutzap_proofs: Vec<Proof> = new_proofs[..nutzap_count].to_vec();
    let change_proofs: Vec<Proof> = new_proofs[nutzap_count..].to_vec();

    // #3008 — this send's own mint-swap fee, derived from conservation
    // (`selected` sums to strictly more than the swap's real outputs by
    // exactly the fee the mint retained) rather than threaded as a separate
    // argument: this works identically whether `finish_send` is driven live
    // (`run_send_worker`, right after `client.swap`) or from a cold-restart
    // WAL resume (`wal_send.rs`, from persisted `new_proofs`) — both already
    // have `selected`/`new_proofs` (nutzap_proofs+change_proofs) in hand.
    // Recorded onto the journal in the `on_signed` fold below, alongside
    // this send's other post-swap facts.
    let selected_total: u64 = selected.iter().map(|stored| stored.proof.amount).sum();
    let new_total: u64 = nutzap_proofs.iter().map(|p| p.amount).sum::<u64>()
        + change_proofs.iter().map(|p| p.amount).sum::<u64>();
    let fee_sats = selected_total.saturating_sub(new_total);

    let Ok(recipient_pk) = nostr::PublicKey::from_hex(&recipient_pubkey) else {
        fail_worker(
            &worker_tx,
            correlation_id,
            ui_codes::OPERATION_FAILED,
            "recipient pubkey is not valid hex".to_string(),
        );
        return;
    };
    let zapped_event_id = target_event_id
        .as_deref()
        .and_then(|id| nostr::EventId::from_hex(id).ok());
    let nutzap_wire_proofs: Vec<NutZapProof> = nutzap_proofs.into_iter().map(Into::into).collect();
    let tags = match nutzap_event_tags(
        &nutzap_wire_proofs,
        &mint,
        &recipient_pk,
        zapped_event_id.as_ref(),
    ) {
        Ok(t) => t,
        Err(e) => {
            fail_worker(
                &worker_tx,
                correlation_id,
                ui_codes::OPERATION_FAILED,
                format!("nutzap tag build failed: {e}"),
            );
            return;
        }
    };

    // #2972 — from here on `mint` only feeds wallet-internal bookkeeping
    // (the ledger fact + `add_proofs` below), never protocol content: the
    // kind:9321 `u` tag above already captured the recipient's own raw
    // mint string via `tags`. Canonicalizing here keeps this send's ledger
    // fact and proof-inventory entry keyed the same way every other
    // deposit/redeem/send for this real mint is, so balances never split
    // into multiple rows for one mint spelled two ways over time.
    let mint = canonicalize_mint_url(&mint);
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id;
    launch_plain_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP61_NUTZAP,
        tags,
        String::new(),
        relays,
        created_at,
        correlation_id,
        move |_tx, _signed| {
            // Fold the swap's real effect into local state ONLY now that the
            // outgoing kind:9321 is actually signed (#2934) — see this
            // file's module docs. `selected` was already removed from the
            // inventory synchronously in `SendNutzapCommand::run` (the
            // reservation that closes the double-tap race — see that call
            // site's comment), so there is nothing left to remove here; only
            // the trail/ledger fold and the change proofs remain.
            let mut s = lock_state(&on_signed_state);
            // Per-proof `MintProbed{Spent}` (not `TokenDeleted`) — a
            // consumed proof may share its kind:7375 token event with OTHER,
            // unselected proofs that are still live; `TokenDeleted` operates
            // at whole-event granularity and would zero those out too.
            for stored in &selected {
                s.ledger.apply(WalletFact::MintProbed {
                    proof: crate::journal::ProofRef::new(stored.proof.c.clone()),
                    verdict: crate::journal::ProofVerdict::Spent,
                });
            }
            if !change_proofs.is_empty() {
                let change_atoms: Vec<ProofAtom> = change_proofs
                    .iter()
                    .map(|p| ProofAtom {
                        proof: crate::journal::ProofRef::new(p.c.clone()),
                        amount_msat: p.amount.saturating_mul(1000),
                    })
                    .collect();
                // No real kind:7375 event exists yet for this change (see
                // module docs' "Deferred" section) — a stable synthetic id
                // keys the trail/balance fold the same way a real event id
                // would.
                s.ledger.apply(WalletFact::TokenAdded {
                    token_event: WalletEventId::new(format!(
                        "pending-change-{}",
                        on_signed_op.as_str()
                    )),
                    mint: MintUrl::new(mint.clone()),
                    unit: WalletUnit::new("sat"),
                    proofs: change_atoms,
                    via: Provenance::Saga(CorrelationId::new(on_signed_op.as_str())),
                });
            }
            s.add_proofs(None, mint, change_proofs);
            let _ = s.journal.record_fee_sats(&on_signed_op, fee_sats);
            let _ = s.transition(&on_signed_op, WalletOperationState::MintSettled);
            let _ = s.transition(&on_signed_op, WalletOperationState::PublishPending);
            // #2960 — settle the send once its kind:9321 is signed (optimistic,
            // mirroring `redeem_worker.rs`'s own `Settled` on its history
            // event's sign). This is the send's terminal transition: it deletes
            // the durable WAL row AND resume payload via `state.rs`'s terminal
            // write-through, so a completed send is never re-driven on a later
            // restart. A send has no external publish-ACK to settle on (its
            // kind:9321 is p-tagged to the recipient, so this account never
            // re-observes it as its own), which is why it settles here rather
            // than on a re-ingested event the way a deposit does.
            let _ = s.transition(&on_signed_op, WalletOperationState::Settled);
        },
    );
}

/// An HTTP failure mid-swap is ambiguous — see `deposit.rs`'s
/// `mark_operation_uncertain` for the identical rationale (`Unknown`, not
/// `Failed`: the mint may have already consumed the inputs). Never restore
/// `selected` here — see [`restore_and_fail`] for the definite-failure twin
/// that does.
fn mark_uncertain(state: &Mutex<CashuWalletState>, operation_id: &WalletOperationId) {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Unknown);
}

/// Undo `SendNutzapCommand::run`'s pre-effect `remove_proofs` reservation and
/// fail closed with `Failed` (not `Unknown`) — used only for a failure BEFORE
/// the mint's `swap` call, where nothing that could have consumed `selected`
/// was ever attempted, so restoring them cannot resurrect an already-spent
/// proof. A failure at or after `swap()` itself must use [`mark_uncertain`]
/// instead and never restore (the mint may have already consumed the inputs
/// even though the response was lost).
fn restore_and_fail(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    selected: &[StoredProof],
    mint: &str,
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) {
    {
        let mut s = lock_state(state);
        for stored in selected {
            s.add_proofs(
                stored.token_event.clone(),
                mint.to_string(),
                vec![stored.proof.clone()],
            );
        }
        let _ = s.transition(operation_id, WalletOperationState::Failed);
    }
    fail_worker(worker_tx, correlation_id, code, reason);
}

fn fail_worker(
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) {
    let token = UiToken::error(code, reason.clone());
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id {
        let _ = worker_tx.send(build_record_action_failure(id, reason));
    }
}
