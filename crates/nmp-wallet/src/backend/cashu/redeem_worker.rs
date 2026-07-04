//! `RedeemNutzap`'s worker-thread half — split out of `redeem.rs` (AGENTS.md
//! file-size discipline). See that file's module docs for the verification
//! this worker's caller (`RedeemNutzapCommand::run`) already performed before
//! spawning it.
//!
//! # Swap-before-redeem ordering
//!
//! 1. Journal pre-record (in `redeem.rs`, before this worker spawns): the
//!    nutzap event is the consumed input, BEFORE the swap HTTP call
//!    (`requires_consumed_inputs_before_mint_request`).
//! 2. This worker signs the P2PK proofs with this wallet's Cashu privkey and
//!    swaps them at the mint for fresh, unlinked proofs — THE value-moving
//!    call. `MintSettled` transition happens only after this succeeds.
//! 3. Publish kind:7375 for the fresh proofs.
//! 4. Publish kind:7376 (history: encrypted direction/amount/created +
//!    plain redeemed `e` + sender `p`) — only once BOTH publishes are
//!    signed does this fold `WalletFact::TokenAdded` (fresh proofs) and
//!    `WalletFact::NutzapRedeemed` into the ledger and mark the nutzap
//!    redeemed in local state. A crash between the swap succeeding and
//!    here loses track of the fresh proofs the same way `PendingDeposit::
//!    minted_proofs`'s in-memory-only window does for deposits (#2910-class
//!    gap, not closed by this PR) — but it can never DOUBLE-count: nothing
//!    marks the nutzap redeemed until the history event is signed.

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_record_action_failure;
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::cashu::{split_amount, MintClient};
use nmp_nip60::kinds::{KIND_NIP60_HISTORY, KIND_NIP60_TOKEN};
use nmp_nip60::nutzap::{sign_p2pk_proof, ReceivedNutZap};
use nmp_signer_iface::SignedEvent;

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, ProofRef, Provenance, PubkeyRef, WalletEventId, WalletFact,
    WalletOperationId, WalletOperationState, WalletUnit,
};

use super::chain::launch_self_encrypted_publish;
use super::deposit::token_event_plaintext;
use super::state::{canonicalize_mint_url, lock_state, CashuWalletState};
use super::ui_codes;

pub(super) struct RedeemWorkerArgs {
    pub(super) worker_tx: CommandSender,
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) nutzap: ReceivedNutZap,
    pub(super) nutzap_wallet_event: WalletEventId,
    pub(super) cashu_sk: nostr::secp256k1::SecretKey,
    pub(super) relays: Vec<String>,
    pub(super) created_at: u64,
    pub(super) correlation_id: Option<String>,
}

pub(super) fn run_redeem_worker(args: RedeemWorkerArgs) {
    let RedeemWorkerArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        nutzap,
        nutzap_wallet_event,
        cashu_sk,
        relays,
        created_at,
        correlation_id,
    } = args;

    let input_proofs: Vec<Proof> = match nutzap
        .proofs
        .iter()
        .map(|np| {
            let proof = Proof {
                amount: np.amount,
                id: np.id.clone(),
                secret: np.secret.clone(),
                c: np.c.clone(),
                dleq: np.dleq.clone(),
                witness: None,
            };
            sign_p2pk_proof(&proof, &cashu_sk)
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(p) => p,
        Err(e) => {
            let _ = lock_state(&state).transition(&operation_id, WalletOperationState::Failed);
            fail_worker(
                &worker_tx,
                correlation_id,
                ui_codes::INVALID_NUTZAP,
                format!("P2PK witness signing failed: {e}"),
            );
            return;
        }
    };

    let client = MintClient::new(&nutzap.mint_url);
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
    let fee = MintClient::compute_fee(input_proofs.len() as u64, keyset.input_fee_ppk);
    let net_total = nutzap.amount_sats.saturating_sub(fee);
    if net_total == 0 {
        // The redemption fee alone consumes (or exceeds) the whole nutzap —
        // `split_amount(0)` yields no outputs, which `prepare_swap_request`
        // would reject anyway, but checking here gives a precise, non-network
        // `Failed` (nothing was sent to the mint) instead of relying on that
        // downstream validation and landing in the ambiguous `Unknown` state.
        let _ = lock_state(&state).transition(&operation_id, WalletOperationState::Failed);
        fail_worker(
            &worker_tx,
            correlation_id,
            ui_codes::INSUFFICIENT_BALANCE,
            "redemption fee consumes the entire nutzap amount".to_string(),
        );
        return;
    }
    let output_amounts = split_amount(net_total);

    // Unlink from the sender — never republish the received proofs
    // themselves (design doc "Privacy And Security"): random secrets, no
    // P2PK lock on the output side.
    let fresh_proofs = match client.swap(input_proofs, output_amounts, None, &keyset) {
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

    // #2931 — the unlinking swap has committed: persist the fresh
    // wallet-owned proofs BEFORE `finish_redeem` runs, so a crash before the
    // kind:7375/7376 are published can re-drive `finish_redeem` from these
    // exact proofs on restart (`wal_redeem.rs`) rather than losing track of
    // them (the redeem-side of the #2910-class gap `finish_redeem`'s own module
    // docs flag).
    {
        let s = lock_state(&state);
        super::wal_redeem::persist_redeem_payload(
            &s,
            &operation_id,
            &nutzap,
            &nutzap_wallet_event,
            &relays,
            Some(fresh_proofs.clone()),
        );
    }

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
        correlation_id,
    });
}

pub(super) struct FinishRedeemArgs {
    pub(super) worker_tx: CommandSender,
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) nutzap: ReceivedNutZap,
    pub(super) nutzap_wallet_event: WalletEventId,
    /// The mint's `swap` response for the received (P2PK-signed) input
    /// proofs — fresh, unlinked, wallet-owned.
    pub(super) fresh_proofs: Vec<Proof>,
    pub(super) relays: Vec<String>,
    pub(super) created_at: u64,
    pub(super) correlation_id: Option<String>,
}

/// Everything after the mint has already committed the unlinking swap:
/// `MintSettled`, publish kind:7375, then kind:7376 (only on ITS `on_signed`
/// does this fold `TokenAdded`/`NutzapRedeemed` and reach `Settled` — see
/// module docs' "Swap-before-redeem ordering"). Split out of
/// `run_redeem_worker` so tests can drive this directly with synthetic
/// post-swap proofs (mirrors `send.rs`'s `finish_send`/`deposit.rs`'s
/// `dispatch_token_event`).
pub(super) fn finish_redeem(args: FinishRedeemArgs) {
    let FinishRedeemArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        nutzap,
        nutzap_wallet_event,
        fresh_proofs,
        relays,
        created_at,
        correlation_id,
    } = args;

    let _ = lock_state(&state).transition(&operation_id, WalletOperationState::MintSettled);

    let mint_for_token = nutzap.mint_url.clone();
    let token_plaintext = token_event_plaintext(&mint_for_token, &fresh_proofs);
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id.clone();
    let worker_tx_for_history = worker_tx.clone();
    let account_for_history = account_pubkey.clone();
    let relays_for_history = relays.clone();
    let correlation_for_history = correlation_id.clone();
    launch_self_encrypted_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP60_TOKEN,
        token_plaintext,
        Vec::new(),
        relays,
        created_at,
        correlation_id,
        move |_tx, token_signed: &SignedEvent| {
            let token_event_id = token_signed.id.clone();
            // `nutzap`/`nutzap_wallet_event`/`fresh_proofs` are only used
            // here — this closure is `FnOnce` (called exactly once by
            // `chain.rs`'s sign continuation) and owns them outright via the
            // `move`, so they move into `PublishHistoryArgs` directly rather
            // than cloning an about-to-be-dropped capture.
            //
            // MintSettled -> PublishPending: the fresh-proofs token event is
            // signed (about to be enqueued); the history publish chained
            // below carries this operation to its terminal `Settled`.
            let _ = lock_state(&on_signed_state)
                .transition(&on_signed_op, WalletOperationState::PublishPending);
            publish_redeem_history(PublishHistoryArgs {
                worker_tx: worker_tx_for_history,
                state: on_signed_state,
                operation_id: on_signed_op,
                account_pubkey: account_for_history,
                nutzap,
                nutzap_wallet_event,
                mint: mint_for_token,
                fresh_proofs,
                token_event_id,
                relays: relays_for_history,
                created_at,
                correlation_id: correlation_for_history,
            });
        },
    );
}

struct PublishHistoryArgs {
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    operation_id: WalletOperationId,
    account_pubkey: String,
    nutzap: ReceivedNutZap,
    nutzap_wallet_event: WalletEventId,
    mint: String,
    fresh_proofs: Vec<Proof>,
    token_event_id: String,
    relays: Vec<String>,
    created_at: u64,
    correlation_id: Option<String>,
}

/// Publish kind:7376 marking `nutzap` redeemed. Runs on the actor thread
/// (called from `launch_self_encrypted_publish`'s `on_signed`, which itself
/// runs there — see `chain.rs`) so it re-enters `launch_self_encrypted_publish`
/// directly rather than needing another worker thread.
fn publish_redeem_history(args: PublishHistoryArgs) {
    let PublishHistoryArgs {
        worker_tx,
        state,
        operation_id,
        account_pubkey,
        nutzap,
        nutzap_wallet_event,
        mint,
        fresh_proofs,
        token_event_id,
        relays,
        created_at,
        correlation_id,
    } = args;

    // #2972 — `mint` already did protocol duty (the fresh kind:7375's
    // plaintext was built from this exact string back in `mint_for_token`);
    // from here it only feeds wallet-internal bookkeeping (the ledger fact +
    // `add_proofs` below), so canonicalize it now so this redeem's balance
    // fold lands under the SAME mint key every other deposit/send/redeem for
    // this real mint uses, rather than fragmenting balances across however
    // many ways senders happen to spell it.
    let mint = canonicalize_mint_url(&mint);

    let (history_plaintext, history_tags) =
        history_plaintext_and_tags(nutzap.amount_sats, &token_event_id, &nutzap);
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id;
    launch_self_encrypted_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP60_HISTORY,
        history_plaintext,
        history_tags,
        relays,
        created_at,
        correlation_id,
        move |_tx, _history_signed: &SignedEvent| {
            // Only NOW — both the fresh token event and the history event
            // are signed — does this become durable balance/redemption
            // state. See module docs' "Swap-before-redeem ordering".
            let mut s = lock_state(&on_signed_state);
            let proof_atoms: Vec<ProofAtom> = fresh_proofs
                .iter()
                .map(|p| ProofAtom {
                    proof: ProofRef::new(p.c.clone()),
                    amount_msat: p.amount.saturating_mul(1000),
                })
                .collect();
            s.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(token_event_id.clone()),
                mint: MintUrl::new(mint.clone()),
                unit: WalletUnit::new("sat"),
                proofs: proof_atoms,
                via: Provenance::Saga(CorrelationId::new(on_signed_op.as_str())),
            });
            s.ledger.apply(WalletFact::NutzapRedeemed {
                nutzap: nutzap_wallet_event,
                amount_msat: nutzap.amount_sats.saturating_mul(1000),
                sender: PubkeyRef::new(nutzap.sender_pubkey.to_hex()),
            });
            s.add_proofs(Some(token_event_id), mint, fresh_proofs);
            let _ = s.transition(&on_signed_op, WalletOperationState::Settled);
        },
    );
}

/// The kind:7376 plaintext (encrypted content) + plain tags for a redeemed
/// nutzap. Mirrors `nmp_nip60::history_event::build_history_event`'s content
/// shape exactly (so a client reading this back with
/// `nmp_nip60::history_event`'s own decode — not written yet, out of this
/// PR's scope — would parse it unchanged) — duplicated rather than called
/// because that function encrypts with a raw `nostr::Keys` (D13: this
/// backend never holds/uses a raw Nostr identity keypair; encryption must go
/// through the signer-transparent NIP-44 port in `chain.rs` instead). Same
/// class of duplication `create_wallet.rs`'s `wallet_config_plaintext`
/// already documents for the analogous kind:17375 case.
fn history_plaintext_and_tags(
    amount_sats: u64,
    created_token_event_id: &str,
    nutzap: &ReceivedNutZap,
) -> (String, Vec<Vec<String>>) {
    let data = vec![
        vec!["direction".to_string(), "in".to_string()],
        vec!["amount".to_string(), amount_sats.to_string()],
        vec![
            "e".to_string(),
            created_token_event_id.to_string(),
            String::new(),
            "created".to_string(),
        ],
    ];
    let plaintext = serde_json::to_string(&data).unwrap_or_else(|_| "[]".to_string());
    let tags = vec![
        vec![
            "e".to_string(),
            nutzap.event_id.to_hex(),
            String::new(),
            "redeemed".to_string(),
        ],
        vec!["p".to_string(), nutzap.sender_pubkey.to_hex()],
    ];
    (plaintext, tags)
}

/// An HTTP failure mid-swap is ambiguous — see `deposit.rs`'s
/// `mark_operation_uncertain` for the identical rationale.
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
