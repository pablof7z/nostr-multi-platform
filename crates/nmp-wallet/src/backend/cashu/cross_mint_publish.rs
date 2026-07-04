//! The target-mint leg + kind:7375 publish for a `CrossMintTransfer` (#3003)
//! — split out of `cross_mint_worker.rs` (AGENTS.md file-size discipline).
//! Shared by the fresh flow (`cross_mint_worker::run_cross_mint_transfer_worker`,
//! once the melt settles) and `cross_mint_resume::ResumeCrossMintTransferCommand`
//! (cold-restart resume, after reconciling an unsettled melt). See
//! `cross_mint`'s module docs for the saga overview.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{build_record_action_failure, build_record_action_success};
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::{MintQuoteState, Proof};
use nmp_nip60::cashu::MintClient;
use nmp_nip60::KIND_NIP60_TOKEN;

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, ProofRef, Provenance, WalletEventId, WalletFact,
    WalletOperationId, WalletOperationKind, WalletOperationState, WalletUnit,
};

use super::chain::{enqueue_signed_publish, launch_self_encrypted_publish};
use super::cross_mint_worker::SendRetry;
use super::deposit::token_event_plaintext;
use super::send::SendNutzapCommand;
use super::state::{canonicalize_mint_url, lock_state, CashuWalletState};

/// Resume (or, from the fresh flow, continue) the target-mint leg once the
/// melt is known to have settled — mint the target's tokens (retry-safe:
/// idempotent while the quote is PAID-not-yet-ISSUED, write-if-absent
/// fenced via `minted_proofs`) and publish the resulting kind:7375. Shared
/// by the fresh flow (`run_cross_mint_transfer_worker`, above) and
/// `cross_mint_resume::ResumeCrossMintTransferCommand` (which passes
/// `on_settled: None`, `standalone_correlation_id: None` — a cold-restart
/// resume never reports a live action result, mirroring
/// `ResumeDepositCommand`/`ResumeSendCommand`).
#[allow(clippy::too_many_arguments)]
pub(super) fn resume_target_mint_leg(
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    account_pubkey: String,
    target_quote_id: String,
    relays: Vec<String>,
    created_at: u64,
    on_settled: Option<SendRetry>,
    standalone_correlation_id: Option<String>,
) {
    let pending = {
        let guard = lock_state(&state);
        guard
            .pending_cross_mint_transfers
            .get(&target_quote_id)
            .cloned()
    };
    let Some(pending) = pending else {
        return;
    };

    if let Some(signed) = pending.signed_token.clone() {
        enqueue_signed_publish(&worker_tx, &signed, relays, None);
        return;
    }

    let proofs = if let Some(proofs) = pending.minted_proofs.clone() {
        proofs
    } else {
        let target_client = MintClient::new(&pending.target_mint);
        // Re-check paid (should always already be true — our own melt just
        // paid it — but never assume; mirrors `deposit`'s `Fresh` resume).
        match target_client.get_mint_quote_status(&target_quote_id) {
            Ok(status) if status.state == MintQuoteState::Paid => {}
            Ok(_) => return,  // not (yet) paid — retryable, no transition
            Err(_) => return, // transport failure — retryable, no transition
        }
        let keyset = match target_client.get_sat_keyset() {
            Ok(k) => k,
            Err(_) => return, // retryable
        };
        let proofs = match target_client.mint_tokens(&target_quote_id, pending.amount_sats, &keyset)
        {
            Ok(p) => p,
            // Retryable — see this file's module docs: if the quote was
            // already ISSUED by an earlier attempt whose response was
            // lost before `minted_proofs` got recorded, those proofs
            // are unrecoverable (the same accepted class of gap
            // `PendingDeposit::minted_proofs` documents for deposits).
            Err(_) => return,
        };
        {
            let mut guard = lock_state(&state);
            if let Some(p) = guard.pending_cross_mint_transfers.get_mut(&target_quote_id) {
                if p.minted_proofs.is_none() {
                    p.minted_proofs = Some(proofs.clone());
                }
            }
            super::cross_mint_resume::persist_cross_mint_payload(&guard, &target_quote_id);
        }
        proofs
    };

    // #3008 — the melt fee this transfer's OWN `CrossMintTransfer` operation
    // recorded (see `cross_mint_worker.rs`'s `melt_fee_consumed`), read back
    // so it (plus `pending.source_mint`) can be copied onto the retried
    // send's journal row below — `0` for a resume/standalone caller that
    // never recorded one (rather than failing the whole leg over a display
    // field).
    let melt_fee_sats = {
        let guard = lock_state(&state);
        guard
            .journal
            .get(&pending.operation_id)
            .and_then(|op| op.recorded_fee_sats)
            .unwrap_or(0)
    };

    dispatch_cross_mint_token_event(
        worker_tx,
        state,
        pending.operation_id,
        target_quote_id,
        pending.target_mint,
        proofs,
        account_pubkey,
        relays,
        created_at,
        on_settled,
        standalone_correlation_id,
        pending.source_mint,
        melt_fee_sats,
    );
}

/// Build the kind:7375 self-encrypted token event for the target-mint
/// `proofs` and launch the encrypt -> sign -> publish chain, folding the
/// real proofs + accepted-mint list on sign, then (fresh-flow only)
/// re-dispatching the original `SendNutzap` or reporting the standalone
/// action's success.
#[allow(clippy::too_many_arguments)]
fn dispatch_cross_mint_token_event(
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    operation_id: WalletOperationId,
    target_quote_id: String,
    target_mint: String,
    proofs: Vec<Proof>,
    account_pubkey: String,
    relays: Vec<String>,
    created_at: u64,
    on_settled: Option<SendRetry>,
    standalone_correlation_id: Option<String>,
    // #3008 — this transfer's own melt source mint + realized melt fee,
    // copied onto the retried send's journal row below (fresh-flow/
    // auto-fallback only — see the `on_settled` branch) so
    // `snapshot.rs::history_row` can surface them without decoding a proof.
    cross_mint_source: String,
    cross_mint_fee_sats: u64,
) {
    let plaintext = token_event_plaintext(&target_mint, &proofs);
    let proof_atoms: Vec<ProofAtom> = proofs
        .iter()
        .map(|p| ProofAtom {
            proof: ProofRef::new(p.c.clone()),
            amount_msat: p.amount.saturating_mul(1000),
        })
        .collect();
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id.clone();
    let target_mint_for_fold = canonicalize_mint_url(&target_mint);
    let account_pubkey_for_retry = account_pubkey.clone();
    launch_self_encrypted_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP60_TOKEN,
        plaintext,
        Vec::new(),
        relays,
        created_at,
        None,
        move |tx, signed| {
            let mut guard = lock_state(&on_signed_state);
            guard.add_proofs(
                Some(signed.id.clone()),
                target_mint_for_fold.clone(),
                proofs,
            );
            guard.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(signed.id.clone()),
                mint: MintUrl::new(target_mint_for_fold.clone()),
                unit: WalletUnit::new("sat"),
                proofs: proof_atoms,
                via: Provenance::Saga(CorrelationId::new(on_signed_op.as_str())),
            });
            // The target mint now genuinely holds this wallet's real
            // balance — make it an accepted mint going forward (if it
            // wasn't already) so `send.rs`'s mutual-mint check finds it on
            // the retried send below, and any future direct send/deposit
            // recognizes it too.
            if !guard
                .mints
                .iter()
                .any(|m| canonicalize_mint_url(m) == target_mint_for_fold)
            {
                guard.mints.push(target_mint_for_fold.clone());
            }
            let _ = guard.transition(&on_signed_op, WalletOperationState::PublishPending);
            let _ = guard.transition(&on_signed_op, WalletOperationState::Settled);
            if let Some(p) = guard.pending_cross_mint_transfers.get_mut(&target_quote_id) {
                p.signed_token = Some(signed.clone());
            }
            super::cross_mint_resume::persist_cross_mint_payload(&guard, &target_quote_id);
            drop(guard);

            if let Some(retry) = on_settled.clone() {
                // Re-dispatch the ORIGINAL nutzap send now that the target
                // mint is funded — a fresh journal operation (this is
                // logically a brand-new `SendNutzap`). Its OPERATION id must
                // be DISTINCT from the original send's: that original
                // operation (keyed by `retry.correlation_id` — see
                // `nutzap_dispatch.rs::start_send_nutzap`) is still sitting
                // in the journal as a terminal `Failed` row (superseded, not
                // removed — see `send.rs`'s fallback comment), so reusing
                // `operation_id_for(&retry.correlation_id, ..)` here would
                // collide with it and `begin_operation_at` would fail closed
                // with `DuplicateOperation` on every retry that carries a
                // correlation id — i.e. always in production (#3008 fix; the
                // pre-fix behavior silently dropped the retry with `began:
                // false` and no failure ever reported). `target_quote_id` is
                // mint-issued and unique per transfer, so keying off it here
                // can never collide with the original send's id OR a
                // different concurrent transfer's retry.
                //
                // `retry.correlation_id` is passed through UNCHANGED on
                // `SendNutzapCommand` below — that is what the caller's
                // one-shot action-result channel is keyed on, entirely
                // separate from this journal operation id.
                let retry_op =
                    WalletOperationId::new(format!("cross-mint-retry-send-{target_quote_id}"));
                let began = {
                    let mut guard = lock_state(&on_signed_state);
                    let ok = guard
                        .begin_operation_at(
                            retry_op.clone(),
                            WalletOperationKind::SendNutzap,
                            created_at,
                        )
                        .is_ok();
                    if ok {
                        let _ = guard.journal.record_amount(&retry_op, retry.amount_sats);
                        let _ = guard.journal.record_cross_mint_origin(
                            &retry_op,
                            cross_mint_source,
                            cross_mint_fee_sats,
                        );
                    }
                    ok
                };
                if began {
                    let _ = tx.send(ActorCommand::Protocol(Box::new(SendNutzapCommand {
                        state: Arc::clone(&on_signed_state),
                        operation_id: retry_op,
                        account_pubkey: account_pubkey_for_retry.clone(),
                        recipient_pubkey: retry.recipient_pubkey,
                        amount_sats: retry.amount_sats,
                        target_event_id: retry.target_event_id,
                        correlation_id: retry.correlation_id,
                    })));
                } else if let Some(id) = retry.correlation_id.clone() {
                    // Never leave the caller's one-shot action-result channel
                    // dangling: the target mint IS funded (the transfer
                    // itself settled), but the retry journal write failed —
                    // report it rather than silently dropping the retry.
                    let _ = tx.send(build_record_action_failure(
                        id,
                        "cross-mint transfer settled, but the re-dispatched send could not begin \
                         (journal error) — the target mint is funded, retry nutzap.send"
                            .to_string(),
                    ));
                }
            } else if let Some(id) = standalone_correlation_id.clone() {
                let result_json = serde_json::json!({
                    "target_mint": target_mint_for_fold,
                    "settled": true,
                })
                .to_string();
                let _ = tx.send(build_record_action_success(id, Some(result_json)));
            }
        },
    );
}
