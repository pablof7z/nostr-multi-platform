//! `WalletDerivedState` — the fold result `WalletLedger` (in `ledger`) keeps
//! alongside the causal trail: which token events are live/tombstoned, which
//! proofs have a reconciled spend verdict, and the balances derived from
//! both.
//!
//! The two `apply_*` guards here are the confluence mechanism: both live
//! `WalletLedger::fold` and restart `WalletLedger::rebuild_from` call the
//! *same* methods, so a `TokenDeleted`/`TokenTombstoned` that logically
//! precedes its matching add stays authoritative regardless of which path,
//! or which order, folded it.

use std::collections::BTreeMap;

use super::fact::{
    DeleteCause, MintUrl, ProofAtom, ProofRef, ProofVerdict, PubkeyRef, WalletEventId, WalletUnit,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WalletBalanceKey {
    pub mint: MintUrl,
    pub unit: WalletUnit,
}

#[derive(Clone, Debug)]
struct TokenRecord {
    mint: MintUrl,
    unit: WalletUnit,
    proofs: Vec<ProofAtom>,
}

/// Rebuildable from scratch by `WalletLedger::rebuild_from` — intentionally
/// not `Serialize`; nothing durable is stored here directly, the durable
/// tier is the protocol's own kind:7375/7376 events.
#[derive(Clone, Debug, Default)]
pub struct WalletDerivedState {
    live_tokens: BTreeMap<WalletEventId, TokenRecord>,
    tombstoned_tokens: BTreeMap<WalletEventId, DeleteCause>,
    proof_to_token: BTreeMap<ProofRef, WalletEventId>,
    proof_verdicts: BTreeMap<ProofRef, ProofVerdict>,
    pub(super) redeemed_nutzaps: BTreeMap<WalletEventId, (u64, PubkeyRef)>,
}

impl WalletDerivedState {
    #[must_use]
    pub fn is_token_live(&self, token_event: &WalletEventId) -> bool {
        self.live_tokens.contains_key(token_event)
    }

    #[must_use]
    pub fn is_token_tombstoned(&self, token_event: &WalletEventId) -> bool {
        self.tombstoned_tokens.contains_key(token_event)
    }

    #[must_use]
    pub fn proof_verdict(&self, proof: &ProofRef) -> Option<ProofVerdict> {
        self.proof_verdicts.get(proof).copied()
    }

    /// Whether a `NutzapRedeemed` fact has already been folded for `nutzap` —
    /// #2917's `RedeemNutzap` checks this before spending anything, so a
    /// retried/duplicate redeem of the same kind:9321 event never
    /// double-counts.
    #[must_use]
    pub fn is_nutzap_redeemed(&self, nutzap: &WalletEventId) -> bool {
        self.redeemed_nutzaps.contains_key(nutzap)
    }

    /// Balance for one mint/unit pair, excluding any proof reconciled as
    /// spent. A single spent proof does not tombstone its whole token event
    /// — the other proofs on that event may still be good.
    #[must_use]
    pub fn balance(&self, mint: &MintUrl, unit: &WalletUnit) -> u64 {
        self.live_tokens
            .values()
            .filter(|record| &record.mint == mint && &record.unit == unit)
            .flat_map(|record| &record.proofs)
            .filter(|atom| self.is_spendable(&atom.proof))
            .map(|atom| atom.amount_msat)
            .sum()
    }

    #[must_use]
    pub fn balances(&self) -> BTreeMap<WalletBalanceKey, u64> {
        let mut totals = BTreeMap::new();
        for record in self.live_tokens.values() {
            for atom in &record.proofs {
                if !self.is_spendable(&atom.proof) {
                    continue;
                }
                *totals
                    .entry(WalletBalanceKey {
                        mint: record.mint.clone(),
                        unit: record.unit.clone(),
                    })
                    .or_insert(0) += atom.amount_msat;
            }
        }
        totals
    }

    fn is_spendable(&self, proof: &ProofRef) -> bool {
        !matches!(self.proof_verdicts.get(proof), Some(ProofVerdict::Spent))
    }

    /// Add a token event as live — unless it is already tombstoned. This is
    /// the confluence guard: an out-of-order `TokenDeleted` that arrived
    /// first must stay authoritative, whether this call comes from live
    /// `fold` or from `rebuild_from` folding durable seeds in an arbitrary
    /// order. Returns whether the token actually became live.
    pub(super) fn apply_token_live(
        &mut self,
        token_event: WalletEventId,
        mint: MintUrl,
        unit: WalletUnit,
        proofs: Vec<ProofAtom>,
    ) -> bool {
        if self.is_token_tombstoned(&token_event) {
            return false;
        }
        for atom in &proofs {
            self.proof_to_token
                .insert(atom.proof.clone(), token_event.clone());
        }
        let became_live = !proofs.is_empty();
        self.live_tokens
            .insert(token_event, TokenRecord { mint, unit, proofs });
        became_live
    }

    /// Tombstone a token event — idempotent and safe to call before the
    /// matching `apply_token_live`, which is exactly what makes delete-before
    /// -add converge instead of no-op.
    pub(super) fn apply_token_tombstone(
        &mut self,
        token_event: WalletEventId,
        cause: DeleteCause,
    ) -> bool {
        let was_live = self.is_token_live(&token_event);
        self.live_tokens.remove(&token_event);
        self.tombstoned_tokens.insert(token_event, cause);
        was_live
    }

    /// Record a mint-reconciliation verdict for one proof. `Spent` is
    /// absorbing: once a proof is reconciled spent, a later `Unspent` or
    /// `Unknown` probe (e.g. a stale or out-of-order mint response) must
    /// never resurrect it into spendable balance again. Returns whether the
    /// proof's spendability actually changed.
    pub(super) fn apply_mint_probe(&mut self, proof: ProofRef, verdict: ProofVerdict) -> bool {
        if matches!(self.proof_verdicts.get(&proof), Some(ProofVerdict::Spent)) {
            return false;
        }
        self.proof_verdicts.insert(proof, verdict);
        matches!(verdict, ProofVerdict::Spent)
    }
}
