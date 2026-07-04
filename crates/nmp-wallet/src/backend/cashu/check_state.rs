//! NUT-07 check-state pass over held proofs (#2977) — the follow-up
//! `ingest.rs`'s module docs flag as deferred: recovery folds a recovered
//! kind:7375's proofs into `state.proofs` from local `del`/dedup confluence
//! alone, never asking the mint itself whether they are still good. A proof
//! spent by another client/device without a corresponding rollover ever
//! reaching this account's relays would sit there forever, and
//! `select_proofs`'s greedy scan would keep re-selecting it — only to fail,
//! every time, at the mint's own swap call (`send_worker.rs`'s
//! `SWAP_FAILED`). This pass closes that gap by reconciling against the mint
//! directly, batched one `/v1/checkstate` call per distinct mint.
//!
//! # Fail-safe: only an affirmative `Spent` ever removes a proof
//!
//! A mint HTTP failure (network, non-2xx, malformed response, or a response
//! whose shape this pass cannot trust) leaves every proof for THAT mint
//! completely untouched — never dropped speculatively. The existing
//! swap-time fail-safe (`send_worker.rs`'s reservation-restore on a
//! `SWAP_FAILED`) remains the backstop for anything this pass missed or
//! could not reach. Never remove a proof this wallet has not been told, by
//! the mint itself, is actually gone.
//!
//! # Off the actor thread (D8)
//!
//! [`run_check_state_pass`] blocks on one `MintClient::check_state` HTTP
//! call per distinct mint this wallet's held proofs span — callers must run
//! it on its own `std::thread`, exactly like `send_worker.rs`/`deposit/quote.rs`
//! spawn their own mint-HTTP work; this function is the thread body, not a
//! spawner itself.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_nip60::cashu::types::ProofSpendState;
use nmp_nip60::cashu::MintClient;

use crate::journal::{ProofRef, ProofVerdict, WalletFact};

use super::state::{canonicalize_mint_url, lock_state, CashuWalletState, StoredProof};

/// Reconcile every currently-held proof against its mint and drop the ones
/// the mint affirmatively reports spent — folding a `WalletFact::MintProbed`
/// per dropped proof (mirrors `send_worker.rs`'s post-swap fold: per-proof,
/// never whole-token-event, since a spent proof may share its kind:7375
/// event with other, still-live proofs) and removing it from the
/// secret-bearing inventory `select_proofs` reads. Blocking — see module
/// docs for why this must run off the actor thread.
pub(super) fn run_check_state_pass(state: &Mutex<CashuWalletState>) {
    let groups = {
        let s = lock_state(state);
        group_by_mint(&s.proofs)
    };

    for (mint, proofs) in groups {
        let secrets: Vec<String> = proofs.iter().map(|p| p.proof.secret.clone()).collect();
        let states = match MintClient::new(&mint).check_state(&secrets) {
            Ok(states) => states,
            Err(_) => {
                // Fail-safe (see module docs) — a network/protocol error
                // for this mint never touches its proofs.
                continue;
            }
        };
        // `MintClient::check_state` already validates the response is
        // exactly as long as `secrets` (and in the same `Y` order) before
        // returning `Ok` — this is a defensive re-check, never trusting
        // that invariant alone from this distance.
        if states.len() != proofs.len() {
            continue;
        }

        let spent: Vec<StoredProof> = proofs
            .into_iter()
            .zip(states)
            .filter(|(_, proof_state)| proof_state.state == ProofSpendState::Spent)
            .map(|(stored, _)| stored)
            .collect();
        if spent.is_empty() {
            continue;
        }

        let mut s = lock_state(state);
        for stored in &spent {
            s.ledger.apply(WalletFact::MintProbed {
                proof: ProofRef::new(stored.proof.c.clone()),
                verdict: ProofVerdict::Spent,
            });
        }
        s.remove_proofs(&spent);
    }
}

/// Group held proofs by canonical mint URL — batches one check-state call
/// per distinct mint rather than one per proof. Canonicalizes again rather
/// than trusting `StoredProof::mint` is already canonical (#2972's own
/// paranoia, see `state.rs`'s `select_proofs` doc comment for the same
/// reasoning applied here).
fn group_by_mint(proofs: &[StoredProof]) -> BTreeMap<String, Vec<StoredProof>> {
    let mut groups: BTreeMap<String, Vec<StoredProof>> = BTreeMap::new();
    for stored in proofs {
        groups
            .entry(canonicalize_mint_url(&stored.mint))
            .or_default()
            .push(stored.clone());
    }
    groups
}

#[cfg(test)]
#[path = "tests/check_state_tests.rs"]
mod tests;
