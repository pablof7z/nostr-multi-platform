//! `CrossMintTransfer`'s worker-thread half (#3003) — split out of
//! `cross_mint.rs` (AGENTS.md file-size discipline), mirroring the
//! `send.rs`/`send_worker.rs` split. Owns the target mint-quote, source
//! melt-quote, reservation/journal writes, and the melt call itself. Once
//! the melt settles, control passes to `cross_mint_publish::resume_target_mint_leg`
//! — the target-mint-leg logic shared with `cross_mint_resume.rs`'s
//! cold-restart counterpart.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_record_action_failure;
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::MeltQuoteState;
use nmp_nip60::cashu::MintClient;

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, ProofRef, ProofVerdict, Provenance, WalletConsumedInput,
    WalletEventId, WalletFact, WalletOperationId, WalletOperationState, WalletUnit,
};

use super::cross_mint_publish::resume_target_mint_leg;
use super::state::{lock_state, CashuWalletState, PendingCrossMintTransfer};
use super::ui_codes;

/// What to do once the transfer settles and mints real proofs at the target
/// — only populated when `send.rs`'s `nutzap.send` auto-fallback triggered
/// this transfer; `None` for the standalone `nmp.wallet.cashu.cross_mint_transfer`
/// action, which reports its own settle/fail via a plain correlation id
/// instead (see [`resume_target_mint_leg`]'s `standalone_correlation_id`).
///
/// NOTE (accepted scope boundary): this retry is only driven from the LIVE,
/// same-process worker chain — it is NOT persisted to the WAL. A crash
/// between the transfer settling and the retried `SendNutzap` completing
/// still finishes the cross-mint transfer correctly on cold restart (see
/// `cross_mint_resume.rs` — no fund loss, no double-spend), but does NOT
/// automatically re-drive the original nutzap send; the caller must retry
/// `nutzap.send` once more, which now succeeds immediately since the target
/// mint is funded. Mirrors the class of accepted gaps this codebase already
/// documents elsewhere (e.g. `deposit/mod.rs`'s "fenced-out attempt still
/// publishes").
#[derive(Clone)]
pub(super) struct SendRetry {
    pub(super) recipient_pubkey: String,
    pub(super) amount_sats: u64,
    pub(super) target_event_id: Option<String>,
    pub(super) correlation_id: Option<String>,
}

pub(super) struct CrossMintWorkerArgs {
    pub(super) worker_tx: CommandSender,
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) target_mint: String,
    pub(super) source_mint: String,
    pub(super) amount_sats: u64,
    pub(super) relays: Vec<String>,
    pub(super) created_at: u64,
    /// This transfer's OWN correlation id — reported on Settle/Fail only
    /// when `on_settled` is `None` (the standalone action path). `send.rs`'s
    /// auto-fallback always passes `None` here (nothing is waiting on the
    /// transfer's own id — the caller is waiting on the ORIGINAL send,
    /// carried in `on_settled` instead).
    pub(super) correlation_id: Option<String>,
    pub(super) on_settled: Option<SendRetry>,
}

/// The full fresh-flow chain: target mint-quote -> source melt-quote ->
/// reserve+journal -> melt -> (on settle) the target-mint leg. Runs entirely
/// on a spawned worker thread (D8 — every step here is mint HTTP).
pub(super) fn run_cross_mint_transfer_worker(args: CrossMintWorkerArgs) {
    let CrossMintWorkerArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        target_mint,
        source_mint,
        amount_sats,
        relays,
        created_at,
        correlation_id,
        on_settled,
    } = args;

    // 1. Target mint-quote. `create_mint_quote` already enforces (via
    // `MintQuoteExpectation{amount: Some(amount_sats), ..}`) that the
    // returned invoice is for EXACTLY `amount_sats` — the guard the design
    // spec calls for ("reject if the returned bolt11 is for more than
    // amount") is strictly subsumed by this exact-match check.
    let target_client = MintClient::new(&target_mint);
    let target_quote = match target_client.create_mint_quote(amount_sats) {
        Ok(q) => q,
        Err(e) => {
            fail_no_funds_moved(
                &state,
                &operation_id,
                &worker_tx,
                &correlation_id,
                ui_codes::TARGET_MINT_QUOTE_FAILED,
                format!("target mint-quote request failed: {e}"),
            );
            return;
        }
    };

    // 2. Source melt-quote, sized against the target's own bolt11 invoice.
    let source_client = MintClient::new(&source_mint);
    let melt_quote = match source_client.create_melt_quote(&target_quote.request) {
        Ok(q) => q,
        Err(e) => {
            fail_no_funds_moved(
                &state,
                &operation_id,
                &worker_tx,
                &correlation_id,
                ui_codes::MELT_QUOTE_FAILED,
                format!("source melt-quote request failed: {e}"),
            );
            return;
        }
    };
    // Defensive: the source mint's own read of the invoice's amount should
    // match what the target mint-quote was created for (same invoice) —
    // reject rather than melt a different amount than was actually
    // requested.
    if melt_quote.amount != amount_sats {
        fail_no_funds_moved(
            &state,
            &operation_id,
            &worker_tx,
            &correlation_id,
            ui_codes::MELT_QUOTE_FAILED,
            format!(
                "source melt-quote amount {} does not match the requested {amount_sats}",
                melt_quote.amount
            ),
        );
        return;
    }
    let Some(total_to_melt) = melt_quote.amount.checked_add(melt_quote.fee_reserve) else {
        fail_no_funds_moved(
            &state,
            &operation_id,
            &worker_tx,
            &correlation_id,
            ui_codes::MELT_QUOTE_FAILED,
            "melt total overflowed u64".to_string(),
        );
        return;
    };

    // 3. Source keyset — fetched BEFORE reservation so a failure here never
    // needs an unreserve step (mirrors the ordering rationale in
    // `send_worker.rs`'s module docs, just shifted earlier since cross-mint
    // has the luxury of not yet having reserved anything at this point).
    let source_keyset = match source_client.get_sat_keyset() {
        Ok(k) => k,
        Err(e) => {
            fail_no_funds_moved(
                &state,
                &operation_id,
                &worker_tx,
                &correlation_id,
                ui_codes::MELT_QUOTE_FAILED,
                format!("source keyset fetch failed: {e}"),
            );
            return;
        }
    };

    // 4. Select + reserve source proofs for the REAL total (amount +
    // fee_reserve) — do NOT split across source mints (v1 scope, per
    // design spec); fail closed if the pre-selected source mint no longer
    // covers it (balance can shift between candidate selection and here).
    let Some((selected, _selected_total)) =
        lock_state(&state).select_proofs(&source_mint, total_to_melt)
    else {
        fail_no_funds_moved(
            &state,
            &operation_id,
            &worker_tx,
            &correlation_id,
            ui_codes::NO_FUNDABLE_SOURCE_MINT,
            "selected source mint no longer covers the melt total".to_string(),
        );
        return;
    };

    let target_quote_id = target_quote.quote.clone();
    let melt_quote_id = melt_quote.quote.clone();
    {
        let mut s = lock_state(&state);
        for stored in &selected {
            let _ = s.record_consumed_input(
                &operation_id,
                WalletConsumedInput {
                    event_id: stored.token_event.clone().unwrap_or_default(),
                    mint: source_mint.clone(),
                    unit: "sat".to_string(),
                    amount: stored.proof.amount,
                },
            );
        }
        if let Err(e) = s.transition(&operation_id, WalletOperationState::MintPending) {
            // Mirrors `send.rs`'s ordering: nothing is removed from the
            // inventory yet at this point, so failing here needs no
            // restore — see this file's module docs / `send.rs`'s own
            // comment on the identical #2953 self-deadlock hazard (release
            // the guard before calling into a helper that re-locks it).
            drop(s);
            fail_no_funds_moved(
                &state,
                &operation_id,
                &worker_tx,
                &correlation_id,
                ui_codes::JOURNAL_ERROR,
                format!("{e:?}"),
            );
            return;
        }
        s.remove_proofs(&selected);
        s.pending_cross_mint_transfers.insert(
            target_quote_id.clone(),
            PendingCrossMintTransfer {
                operation_id: operation_id.clone(),
                target_mint: target_mint.clone(),
                source_mint: source_mint.clone(),
                amount_sats,
                target_quote_id: target_quote_id.clone(),
                melt_quote_id: melt_quote_id.clone(),
                source_selected: selected.clone(),
                melt_settled: false,
                minted_proofs: None,
                signed_token: None,
                chain_started_at: Some(created_at),
            },
        );
        // The money-safety invariant: the consumed inputs + melt_quote_id
        // are now durably recorded BEFORE the melt HTTP call below.
        super::cross_mint_resume::persist_cross_mint_payload(&s, &target_quote_id);
    }

    // 5. THE irreversible call. From here on, a transport-level failure
    // must NEVER restore `selected` — the Lightning payment may have
    // already left the source mint even if this response was lost.
    let melt_result = source_client.melt(
        &melt_quote_id,
        melt_quote.fee_reserve,
        selected.iter().map(|s| s.proof.clone()).collect(),
        &source_keyset,
    );
    let melt_response = match melt_result {
        Ok(r) => r,
        Err(e) => {
            mark_melt_unknown(
                &state,
                &operation_id,
                &worker_tx,
                &correlation_id,
                format!("melt request failed: {e}"),
            );
            return;
        }
    };
    if melt_response.response.state != MeltQuoteState::Paid {
        // A definite HTTP success, but the mint itself reports the payment
        // still pending/unpaid — genuinely ambiguous in practice (the
        // payment could still complete asynchronously); reconciled only via
        // a later resume's `get_melt_quote_status`, never assumed here.
        mark_melt_unknown(
            &state,
            &operation_id,
            &worker_tx,
            &correlation_id,
            format!(
                "melt quote state after melt() is {:?}, not PAID",
                melt_response.response.state
            ),
        );
        return;
    }

    // Melt settled: fold the source-side effect (consumed inputs already
    // removed at reservation time; credit any NUT-08 change) and advance.
    {
        let mut s = lock_state(&state);
        for stored in &selected {
            s.ledger.apply(WalletFact::MintProbed {
                proof: ProofRef::new(stored.proof.c.clone()),
                verdict: ProofVerdict::Spent,
            });
        }
        if !melt_response.change.is_empty() {
            let change_atoms: Vec<ProofAtom> = melt_response
                .change
                .iter()
                .map(|p| ProofAtom {
                    proof: ProofRef::new(p.c.clone()),
                    amount_msat: p.amount.saturating_mul(1000),
                })
                .collect();
            s.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(format!(
                    "pending-change-{}",
                    operation_id.as_str()
                )),
                mint: MintUrl::new(source_mint.clone()),
                unit: WalletUnit::new("sat"),
                proofs: change_atoms,
                via: Provenance::Saga(CorrelationId::new(operation_id.as_str())),
            });
        }
        s.add_proofs(None, source_mint.clone(), melt_response.change);
        if let Some(p) = s.pending_cross_mint_transfers.get_mut(&target_quote_id) {
            p.melt_settled = true;
        }
        let _ = s.transition(&operation_id, WalletOperationState::MintSettled);
        super::cross_mint_resume::persist_cross_mint_payload(&s, &target_quote_id);
    }

    resume_target_mint_leg(
        worker_tx,
        state,
        account_pubkey,
        target_quote_id,
        relays,
        created_at,
        on_settled,
        correlation_id,
    );
}

/// Report a definite, no-funds-moved failure: transition to `Failed` (no
/// restore needed — nothing was ever reserved) and surface the error.
fn fail_no_funds_moved(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    worker_tx: &CommandSender,
    correlation_id: &Option<String>,
    code: &'static str,
    reason: String,
) {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Failed);
    fail_worker(worker_tx, correlation_id.clone(), code, reason);
}

/// The melt's outcome is ambiguous — transition to `Unknown` (never
/// restore the reserved source proofs; they may already be spent) and
/// surface a retryable-via-resume error.
fn mark_melt_unknown(
    state: &Mutex<CashuWalletState>,
    operation_id: &WalletOperationId,
    worker_tx: &CommandSender,
    correlation_id: &Option<String>,
    reason: String,
) {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Unknown);
    fail_worker(
        worker_tx,
        correlation_id.clone(),
        ui_codes::MELT_UNKNOWN,
        reason,
    );
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
