//! `SendNutzap` intent -> P2PK-lock proofs to a recipient's Cashu pubkey at a
//! mutually-trusted mint, publish kind:9321 (#2917, epic #2864 W8).
//!
//! # Recipient resolution — cache-read-or-fail-closed
//!
//! This wallet has no read interest for an arbitrary recipient's kind:10019
//! by default (unlike the active account's own self-authored events —
//! `interests.rs`). `ctx.latest_author_kind` is a POINT-IN-TIME cache read
//! (see its doc comment), not a fetch: a recipient this account has never
//! observed a kind:10019 from fails closed here immediately. There is no
//! async fetch-then-resume in this PR — see the module docs on
//! `CachedEventLookup`'s trait definition for why (no generic mechanism
//! exists yet to resume a `ProtocolCommand` when a one-shot interest's event
//! arrives; building one is out of this ticket's `nmp-wallet`/`nmp-nip60`
//! scope). The caller (action retry, or a future observe-then-retry UX) is
//! responsible for trying again once the recipient's info has actually
//! arrived through the ordinary kernel read path.
//!
//! # Deferred: republishing kind:7375/kind:5 for spent/change proofs
//!
//! On success this updates the LOCAL proof inventory (removes the spent
//! proofs, adds the change proofs) and the ledger's ref-only facts, so this
//! wallet's own balance is correct immediately. It does NOT publish a
//! replacement kind:7375 token event for the change proofs or a NIP-09
//! deletion of the now-partially-spent token event(s) — mirrors
//! `deposit.rs`'s own documented "kind:7376 not wired" deferral (same class
//! of gap: a session-local wallet has correct state; another NIP-60 client
//! reading this wallet's relays sees a stale kind:7375 until a follow-up
//! wires the republish). Tracked as a fast-follow, not a silent gap.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    build_record_action_failure, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::cashu::{split_amount, MintClient};
use nmp_nip60::kinds::{KIND_NIP61_NUTZAP, KIND_NIP61_NUTZAP_INFO};
use nmp_nip60::nutzap::{decode_nutzap_info_fields, nutzap_event_tags, p2pk_secret, NutZapProof};

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, Provenance, WalletConsumedInput, WalletEventId, WalletFact,
    WalletOperationId, WalletOperationState, WalletUnit,
};

use super::chain::launch_plain_publish;
use super::state::{lock_state, CashuWalletState, StoredProof};
use super::ui_codes;

pub(super) struct SendNutzapCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) recipient_pubkey: String,
    pub(super) amount_sats: u64,
    pub(super) target_event_id: Option<String>,
    pub(super) correlation_id: Option<String>,
}

impl fmt::Debug for SendNutzapCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendNutzapCommand")
            .field("operation_id", &self.operation_id.as_str())
            .field("amount_sats", &self.amount_sats)
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for SendNutzapCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            recipient_pubkey,
            amount_sats,
            target_event_id,
            correlation_id,
        } = *self;

        let Some(info_event) = ctx.latest_author_kind(&recipient_pubkey, KIND_NIP61_NUTZAP_INFO)
        else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_NUTZAP_INFO,
                "recipient has no cached kind:10019 nutzap info".to_string(),
            );
        };
        let recipient_info = decode_nutzap_info_fields(&info_event.tags);
        if recipient_info.relays.is_empty() {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_RELAYS,
                "recipient's kind:10019 lists no relay".to_string(),
            );
        }
        let Some(recipient_cashu_pubkey) = recipient_info.cashu_pubkey.clone() else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_RECIPIENT_P2PK,
                "recipient's kind:10019 has no Cashu P2PK pubkey".to_string(),
            );
        };

        // Use ONLY a mint the recipient lists that this wallet also accepts
        // (the exact `u` tag URL comes from the recipient's own list, never
        // rewritten) — design doc "Relay Acquisition"/"NIP-61 Event Rules".
        let our_mints = lock_state(&state).mints.clone();
        let Some(mint) = recipient_info
            .mints
            .iter()
            .find(|m| our_mints.contains(m))
            .cloned()
        else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::NO_TRUSTED_MINT,
                "no mint the recipient accepts is also accepted by this wallet".to_string(),
            );
        };

        let Some((selected, selected_total)) =
            lock_state(&state).select_proofs(&mint, amount_sats)
        else {
            return fail(
                ctx,
                &state,
                &operation_id,
                correlation_id,
                ui_codes::INSUFFICIENT_BALANCE,
                "insufficient balance at the trusted mint".to_string(),
            );
        };

        // Pre-effect record — money-safety: these inputs are about to be
        // consumed by a mint swap BEFORE the HTTP call goes out.
        {
            let mut s = lock_state(&state);
            for stored in &selected {
                let _ = s.journal.record_consumed_input(
                    &operation_id,
                    WalletConsumedInput {
                        event_id: stored.token_event.clone().unwrap_or_default(),
                        mint: mint.clone(),
                        unit: "sat".to_string(),
                        amount: stored.proof.amount,
                    },
                );
            }
            if let Err(e) = s.transition(&operation_id, WalletOperationState::MintPending) {
                return fail(
                    ctx,
                    &state,
                    &operation_id,
                    correlation_id,
                    ui_codes::JOURNAL_ERROR,
                    format!("{e:?}"),
                );
            }
        }

        let relays = recipient_info.relays.clone();
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            run_send_worker(SendWorkerArgs {
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
            });
        });
        Ok(())
    }
}

struct SendWorkerArgs {
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    operation_id: WalletOperationId,
    account_pubkey: String,
    recipient_pubkey: String,
    mint: String,
    selected: Vec<StoredProof>,
    selected_total: u64,
    amount_sats: u64,
    recipient_cashu_pubkey: String,
    target_event_id: Option<String>,
    relays: Vec<String>,
    created_at: u64,
    correlation_id: Option<String>,
}

fn run_send_worker(args: SendWorkerArgs) {
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
            mark_uncertain(&state, &operation_id);
            fail_worker(
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
        // `Unknown`) is honest: nothing external happened.
        let _ = lock_state(&state).transition(&operation_id, WalletOperationState::Failed);
        fail_worker(
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

/// Everything after the mint has already committed the swap: fold local
/// state, build kind:9321, sign, publish. Split out of `run_send_worker` so
/// tests can drive this directly with synthetic post-swap proofs — the DHKE
/// blind/unblind math itself is `nmp-nip60`'s own already-tested surface
/// (mirrors `deposit.rs`'s `dispatch_token_event` split for the identical
/// reason).
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

    // Fold the swap's real effect into local state BEFORE building the
    // outgoing kind:9321 — the mint has already committed the swap; a crash
    // from here on must not lose track of which proofs are spent/live.
    {
        let mut s = lock_state(&state);
        s.remove_proofs(&selected);
        // Per-proof `MintProbed{Spent}` (not `TokenDeleted`) — a consumed
        // proof may share its kind:7375 token event with OTHER, unselected
        // proofs that are still live; `TokenDeleted` operates at whole-event
        // granularity and would zero those out too.
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
            // No real kind:7375 event exists yet for this change (see module
            // docs' "Deferred" section) — a stable synthetic id keys the
            // trail/balance fold the same way a real event id would.
            s.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(format!("pending-change-{}", operation_id.as_str())),
                mint: MintUrl::new(mint.clone()),
                unit: WalletUnit::new("sat"),
                proofs: change_atoms,
                via: Provenance::Saga(CorrelationId::new(operation_id.as_str())),
            });
        }
        s.add_proofs(None, mint.clone(), change_proofs);
        let _ = s.transition(&operation_id, WalletOperationState::MintSettled);
    }

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
            let mut s = lock_state(&on_signed_state);
            let _ = s.transition(&on_signed_op, WalletOperationState::PublishPending);
        },
    );
}

/// An HTTP failure mid-swap is ambiguous — see `deposit.rs`'s
/// `mark_operation_uncertain` for the identical rationale (`Unknown`, not
/// `Failed`: the mint may have already consumed the inputs).
fn mark_uncertain(state: &Mutex<CashuWalletState>, operation_id: &WalletOperationId) {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Unknown);
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

fn fail(
    ctx: &ProtocolCommandContext<'_>,
    state: &Arc<Mutex<CashuWalletState>>,
    operation_id: &WalletOperationId,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) -> Result<(), ProtocolCommandError> {
    let _ = lock_state(state).transition(operation_id, WalletOperationState::Failed);
    super::report_pre_dispatch_failure(ctx, &correlation_id, code, reason);
    Ok(())
}
