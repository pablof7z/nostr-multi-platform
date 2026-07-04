//! Building and folding the kind:7375 self-encrypted token event for a
//! completed Cashu deposit: [`dispatch_token_event`] launches the
//! encrypt -> sign -> publish chain and, in its `on_signed` closure, folds the
//! minted proofs into the ledger + spendable inventory (fenced on the lease —
//! see [`super::complete`] and [`PendingDeposit`](super::super::state::PendingDeposit)).
//! [`token_event_plaintext`] is the shared kind:7375 content shape (also
//! reused by `redeem_worker.rs`'s fresh-proofs publish).

use std::sync::{Arc, Mutex};

use nmp_core::CommandSender;
use nmp_nip60::cashu::types::Proof;
use nmp_nip60::KIND_NIP60_TOKEN;
use nmp_signer_iface::SignedEvent;

use crate::journal::{
    CorrelationId, MintUrl, ProofAtom, ProofRef, Provenance, WalletEventId, WalletFact,
    WalletOperationId, WalletOperationState, WalletUnit,
};

use super::super::chain::launch_self_encrypted_publish;
use super::super::state::{canonicalize_mint_url, lock_state, CashuWalletState};

/// Build the kind:7375 self-encrypted token event for freshly minted `proofs`
/// and launch the encrypt -> sign -> publish chain. Factored out of `run`'s
/// worker closure so it is directly callable with synthetic proofs — no live
/// mint or DHKE math needed to test the ledger/journal wiring this function
/// owns (the DHKE unblind+verify math itself is `nmp-nip60`'s own tested
/// surface; this function only calls it).
#[allow(clippy::too_many_arguments)]
pub(in crate::backend::cashu) fn dispatch_token_event(
    worker_tx: CommandSender,
    state: Arc<Mutex<CashuWalletState>>,
    operation_id: WalletOperationId,
    quote_id: String,
    mint: String,
    proofs: Vec<Proof>,
    account_pubkey: String,
    relays: Vec<String>,
    created_at: u64,
    correlation_id: Option<String>,
) {
    let plaintext = token_event_plaintext(&mint, &proofs);
    // ProofRef is the proof's public `C` value (never the secret) — matches
    // `ProofRef`'s own doc comment. `amount_msat` here is sats*1000: the
    // ledger's `ProofAtom` field is msat-denominated (future unit-agnostic
    // room); the "sat" unit this deposit flow uses converts at the edge.
    let proof_atoms: Vec<ProofAtom> = proofs
        .iter()
        .map(|p| ProofAtom {
            proof: ProofRef::new(p.c.clone()),
            amount_msat: p.amount.saturating_mul(1000),
        })
        .collect();
    let on_signed_state = Arc::clone(&state);
    let on_signed_op = operation_id.clone();
    // #2972 — `mint` already did protocol duty (`plaintext` above was built
    // from the exact string this deposit was requested against); from here
    // it only feeds wallet-internal bookkeeping (the ledger fact + proof
    // inventory below), so canonicalize it now so this deposit's balance
    // fold lands under the SAME mint key every other deposit/send/redeem for
    // this real mint uses, rather than fragmenting balances across however
    // many ways this mint got typed/observed over time.
    let mint_for_fact = canonicalize_mint_url(&mint);
    launch_self_encrypted_publish(
        worker_tx,
        account_pubkey,
        KIND_NIP60_TOKEN,
        plaintext,
        Vec::new(),
        relays,
        created_at,
        correlation_id,
        move |_tx, signed: &SignedEvent| {
            let mut guard = lock_state(&on_signed_state);
            // Fencing check (see `clear_chain_lease`'s doc comment on why
            // `created_at` is a safe, always-distinct-on-takeover token): if
            // some OTHER, newer attempt has since taken over this quote_id's
            // lease (this one was merely slow, not dead, when it got
            // presumed abandoned), applying these mutations now would
            // double-fold the SAME real proofs the newer attempt's own chain
            // already did — sign the same underlying proof set into two
            // different token events. Fail closed: skip the ledger/proof
            // mutation entirely rather than double-count. The kind:7375
            // event this chain is about to publish anyway still lands on
            // the relay with the real proofs inside it — recovering it into
            // this backend's state is the same durable-reconciliation gap
            // #2910 already tracks (kind:7375 cold-start reconciliation is
            // not implemented today — see `mod.rs`'s module docs), not a
            // NEW loss this fencing introduces.
            if guard.pending_deposits.get(&quote_id).and_then(|p| p.chain_started_at)
                != Some(created_at)
            {
                return;
            }
            // #2917 — the real, secret-bearing proofs this deposit minted
            // must land in the spendable inventory (`select_proofs` is what
            // `SendNutzap`/`RedeemNutzap` draw from), not just the ledger's
            // ref-only `TokenAdded` fact below. Both are written from the
            // SAME `proofs`/`signed.id`, so the inventory and the ledger's
            // aggregate balance never drift apart.
            guard.add_proofs(Some(signed.id.clone()), mint_for_fact.clone(), proofs);
            guard.ledger.apply(WalletFact::TokenAdded {
                token_event: WalletEventId::new(signed.id.clone()),
                mint: MintUrl::new(mint_for_fact),
                unit: WalletUnit::new("sat"),
                proofs: proof_atoms,
                via: Provenance::Saga(CorrelationId::new(on_signed_op.as_str())),
            });
            // `Unknown` (from `mark_operation_uncertain`) allows this
            // transition same as `MintSettled` does — see the state's own
            // `can_transition_to` table — so a retry that recovered via
            // `minted_proofs` still reaches `PublishPending` normally.
            let _ = guard.transition(&on_signed_op, WalletOperationState::PublishPending);
            // NOT removed (#2910/#2923 money-safety fix): the publish this
            // closure hands off to right after returning is fire-and-forget
            // (no ACK loops back here — see `chain.rs`'s module docs) and
            // may still fail (no relay resolves, a relay round-trip errors).
            // Cache the signed event instead so a retry for this exact
            // `quote_id` republishes it rather than hitting `UNKNOWN_QUOTE`
            // with real, already-credited proofs stranded — see
            // `PendingDeposit::signed_token`'s doc comment.
            if let Some(pending) = guard.pending_deposits.get_mut(&quote_id) {
                pending.signed_token = Some(signed.clone());
                // The chain succeeded — no attempt is in flight anymore
                // (see `PendingDeposit::chain_started_at`'s doc comment).
                pending.chain_started_at = None;
            }
            // Durable WAL payload write (PR-2 of #2910): persist the signed
            // kind:7375 so a crash after signing but before the publish ACKs
            // resumes by republishing this EXACT cached event on restart (see
            // `wal_payload::restore_deposits` / `ResumeDepositCommand`), never
            // re-mints and never re-signs. The saga row already moved to
            // `PublishPending` above (write-through by `transition`); this adds
            // the secret-bearing token blob the row deliberately omits.
            super::super::wal_payload::persist_deposit_payload(&guard, &quote_id);
        },
    );
}

/// The kind:7375 token event's NIP-44-encrypted content shape — shared with
/// `redeem_worker.rs`'s own fresh-proofs publish (#2917 W9), which reuses
/// this exact function rather than duplicating it (both are pure JSON
/// construction, no signer/raw-key involvement, so unlike
/// `create_wallet.rs`'s `wallet_config_plaintext`/`redeem_worker.rs`'s
/// `history_plaintext_and_tags` there is no reason for two copies).
pub(in crate::backend::cashu) fn token_event_plaintext(mint: &str, proofs: &[Proof]) -> String {
    serde_json::json!({
        "mint": mint,
        "proofs": proofs,
        "del": Vec::<String>::new(),
    })
    .to_string()
}
