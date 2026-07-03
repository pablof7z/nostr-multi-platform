//! `WalletLedger` — the event-sourced reducer: derived state (proof set /
//! balance) is the fold over the `WalletFact` stream, kept alongside the two
//! causal-trail views from `trail`.
//!
//! Confluence is the load-bearing property here: the terminal derived state
//! must be order-insensitive even though the trail itself is order-sensitive.
//! A `TokenDeleted` fact arriving before the `TokenAdded` it deletes must
//! tombstone the token event id up front, so the later `TokenAdded` sees the
//! tombstone and refuses to resurrect it — see `ledger_state`'s `apply_*`
//! guards, which both `fold` (live) and `rebuild_from` (restart) share.

use std::sync::Arc;

use super::fact::{
    DeleteCause, MintUrl, ProofAtom, PubkeyRef, WalletEventId, WalletFact, WalletUnit,
};
use super::trail::{WalletCauseIndex, WalletDeltaRing};

pub use super::ledger_state::{WalletBalanceKey, WalletDerivedState};

/// Coarse pre-session seeds folded by `WalletLedger::rebuild_from`. These
/// come from durable kind:7375 token state and kind:7376 history — the
/// protocol's own durable causal tier — not from a bespoke local trail store.
pub enum HistoryFactSeed {
    TokenLive {
        token_event: WalletEventId,
        mint: MintUrl,
        unit: WalletUnit,
        proofs: Vec<ProofAtom>,
    },
    TokenTombstoned {
        token_event: WalletEventId,
        cause: DeleteCause,
    },
    NutzapRedeemed {
        nutzap: WalletEventId,
        amount_msat: u64,
        sender: PubkeyRef,
    },
}

pub struct WalletApplySummary {
    pub sequence: u64,
    pub balances_changed: bool,
}

pub struct WalletLedger {
    state: WalletDerivedState,
    ring: WalletDeltaRing,
    causes: WalletCauseIndex,
}

impl WalletLedger {
    #[must_use]
    pub fn new(ring_capacity: usize) -> Self {
        Self {
            state: WalletDerivedState::default(),
            ring: WalletDeltaRing::with_capacity(ring_capacity),
            causes: WalletCauseIndex::default(),
        }
    }

    /// Fold one live fact into derived state, then record it into both trail
    /// views. Order matters for the trail (`sequence`), not for the terminal
    /// derived state (confluence).
    pub fn apply(&mut self, fact: WalletFact) -> WalletApplySummary {
        let balances_changed = self.fold(&fact);
        let fact = Arc::new(fact);
        let entry = self.ring.push(Arc::clone(&fact));
        self.index_causes(&fact);
        WalletApplySummary {
            sequence: entry.sequence,
            balances_changed,
        }
    }

    fn fold(&mut self, fact: &WalletFact) -> bool {
        match fact {
            WalletFact::TokenAdded {
                token_event,
                mint,
                unit,
                proofs,
                ..
            } => self.state.apply_token_live(
                token_event.clone(),
                mint.clone(),
                unit.clone(),
                proofs.clone(),
            ),
            WalletFact::TokenDeleted { token_event, cause } => self
                .state
                .apply_token_tombstone(token_event.clone(), cause.clone()),
            WalletFact::MintProbed { proof, verdict } => {
                self.state.apply_mint_probe(proof.clone(), *verdict)
            }
            WalletFact::NutzapRedeemed {
                nutzap,
                amount_msat,
                sender,
            } => {
                self.state
                    .redeemed_nutzaps
                    .insert(nutzap.clone(), (*amount_msat, sender.clone()));
                false
            }
            WalletFact::SagaTransition { .. } | WalletFact::StateRebuilt { .. } => false,
        }
    }

    fn index_causes(&mut self, fact: &Arc<WalletFact>) {
        match fact.as_ref() {
            WalletFact::TokenAdded {
                token_event,
                proofs,
                ..
            } => {
                self.causes
                    .record_event_cause(token_event.clone(), Arc::clone(fact));
                for atom in proofs {
                    self.causes
                        .record_proof_cause(atom.proof.clone(), Arc::clone(fact));
                }
            }
            WalletFact::TokenDeleted { token_event, .. } => {
                self.causes
                    .record_event_cause(token_event.clone(), Arc::clone(fact));
            }
            WalletFact::MintProbed { proof, .. } => {
                self.causes
                    .record_proof_cause(proof.clone(), Arc::clone(fact));
            }
            WalletFact::NutzapRedeemed { nutzap, .. } => {
                self.causes
                    .record_event_cause(nutzap.clone(), Arc::clone(fact));
            }
            WalletFact::SagaTransition { op, .. } => {
                self.causes
                    .record_correlation_cause(op.clone(), Arc::clone(fact));
            }
            WalletFact::StateRebuilt { from } => {
                for event in from {
                    self.causes
                        .record_event_cause(event.clone(), Arc::clone(fact));
                }
            }
        }
    }

    /// Rebuild a ledger from durable seeds after restart, recording exactly
    /// one `StateRebuilt` genesis fact. This never reads a prior ring — the
    /// ring is a diagnostic surface, not a rebuild authority.
    #[must_use]
    pub fn rebuild_from(
        ring_capacity: usize,
        seeds: impl IntoIterator<Item = HistoryFactSeed>,
    ) -> Self {
        let mut state = WalletDerivedState::default();
        let mut touched = Vec::new();

        for seed in seeds {
            match seed {
                HistoryFactSeed::TokenLive {
                    token_event,
                    mint,
                    unit,
                    proofs,
                } => {
                    // Same confluence guard as live `fold`: seeds can arrive
                    // in any order (a tombstone seed before its matching
                    // live seed must still win), so this must not bypass it.
                    state.apply_token_live(token_event.clone(), mint, unit, proofs);
                    touched.push(token_event);
                }
                HistoryFactSeed::TokenTombstoned { token_event, cause } => {
                    state.apply_token_tombstone(token_event.clone(), cause);
                    touched.push(token_event);
                }
                HistoryFactSeed::NutzapRedeemed {
                    nutzap,
                    amount_msat,
                    sender,
                } => {
                    state
                        .redeemed_nutzaps
                        .insert(nutzap.clone(), (amount_msat, sender));
                    touched.push(nutzap);
                }
            }
        }

        let mut ring = WalletDeltaRing::with_capacity(ring_capacity);
        let mut causes = WalletCauseIndex::default();
        let genesis = Arc::new(WalletFact::StateRebuilt {
            from: touched.clone(),
        });
        ring.push(Arc::clone(&genesis));
        for event in touched {
            causes.record_event_cause(event, Arc::clone(&genesis));
        }

        Self {
            state,
            ring,
            causes,
        }
    }

    #[must_use]
    pub fn state(&self) -> &WalletDerivedState {
        &self.state
    }

    #[must_use]
    pub fn ring(&self) -> &WalletDeltaRing {
        &self.ring
    }

    #[must_use]
    pub fn causes(&self) -> &WalletCauseIndex {
        &self.causes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::fact::{ProofRef, ProofVerdict, Provenance};

    fn mint() -> MintUrl {
        MintUrl::new("https://mint.example")
    }

    fn unit() -> WalletUnit {
        WalletUnit::new("sat")
    }

    #[test]
    fn confluence_delete_before_add_still_tombstones() {
        let mut in_order = WalletLedger::new(16);
        in_order.apply(WalletFact::TokenAdded {
            token_event: WalletEventId::new("token-1"),
            mint: mint(),
            unit: unit(),
            proofs: vec![ProofAtom {
                proof: ProofRef::new("proof-1"),
                amount_msat: 100,
            }],
            via: Provenance::MintRollover,
        });
        in_order.apply(WalletFact::TokenDeleted {
            token_event: WalletEventId::new("token-1"),
            cause: DeleteCause::Nip09Delete {
                by: WalletEventId::new("delete-1"),
            },
        });

        let mut out_of_order = WalletLedger::new(16);
        out_of_order.apply(WalletFact::TokenDeleted {
            token_event: WalletEventId::new("token-1"),
            cause: DeleteCause::Nip09Delete {
                by: WalletEventId::new("delete-1"),
            },
        });
        out_of_order.apply(WalletFact::TokenAdded {
            token_event: WalletEventId::new("token-1"),
            mint: mint(),
            unit: unit(),
            proofs: vec![ProofAtom {
                proof: ProofRef::new("proof-1"),
                amount_msat: 100,
            }],
            via: Provenance::MintRollover,
        });

        assert_eq!(in_order.state().balance(&mint(), &unit()), 0);
        assert_eq!(out_of_order.state().balance(&mint(), &unit()), 0);
        assert!(in_order
            .state()
            .is_token_tombstoned(&WalletEventId::new("token-1")));
        assert!(out_of_order
            .state()
            .is_token_tombstoned(&WalletEventId::new("token-1")));
        assert!(!out_of_order
            .state()
            .is_token_live(&WalletEventId::new("token-1")));
    }

    #[test]
    fn mint_probe_found_spent_excludes_only_that_proof_from_balance() {
        let mut ledger = WalletLedger::new(16);
        ledger.apply(WalletFact::TokenAdded {
            token_event: WalletEventId::new("token-1"),
            mint: mint(),
            unit: unit(),
            proofs: vec![
                ProofAtom {
                    proof: ProofRef::new("proof-a"),
                    amount_msat: 10,
                },
                ProofAtom {
                    proof: ProofRef::new("proof-b"),
                    amount_msat: 5,
                },
            ],
            via: Provenance::MintRollover,
        });
        assert_eq!(ledger.state().balance(&mint(), &unit()), 15);

        let summary = ledger.apply(WalletFact::MintProbed {
            proof: ProofRef::new("proof-a"),
            verdict: ProofVerdict::Spent,
        });

        assert!(summary.balances_changed);
        assert_eq!(ledger.state().balance(&mint(), &unit()), 5);
        assert_eq!(
            ledger.state().proof_verdict(&ProofRef::new("proof-a")),
            Some(ProofVerdict::Spent)
        );
        assert!(ledger
            .causes()
            .last_proof_cause(&ProofRef::new("proof-a"))
            .is_some());
    }

    #[test]
    fn rebuild_from_records_one_state_rebuilt_genesis_fact() {
        let ledger = WalletLedger::rebuild_from(
            16,
            [HistoryFactSeed::TokenLive {
                token_event: WalletEventId::new("token-1"),
                mint: mint(),
                unit: unit(),
                proofs: vec![ProofAtom {
                    proof: ProofRef::new("proof-1"),
                    amount_msat: 42,
                }],
            }],
        );

        assert_eq!(ledger.ring().len(), 1);
        assert!(matches!(
            ledger.ring().iter().next().unwrap().fact.as_ref(),
            WalletFact::StateRebuilt { .. }
        ));
        assert_eq!(ledger.state().balance(&mint(), &unit()), 42);

        let cause = ledger
            .causes()
            .last_event_cause(&WalletEventId::new("token-1"))
            .expect("rebuilt token has an honest cause");
        assert!(matches!(cause, WalletFact::StateRebuilt { .. }));
    }

    /// Regression: `rebuild_from` must apply the same confluence guard as
    /// live `fold` — a `TokenTombstoned` seed for an event must stay
    /// authoritative even when a `TokenLive` seed for the same event is
    /// folded afterward (durable seed order is not guaranteed).
    #[test]
    fn rebuild_from_is_confluent_regardless_of_seed_order() {
        let tombstone_first = WalletLedger::rebuild_from(
            16,
            [
                HistoryFactSeed::TokenTombstoned {
                    token_event: WalletEventId::new("token-1"),
                    cause: DeleteCause::Nip09Delete {
                        by: WalletEventId::new("delete-1"),
                    },
                },
                HistoryFactSeed::TokenLive {
                    token_event: WalletEventId::new("token-1"),
                    mint: mint(),
                    unit: unit(),
                    proofs: vec![ProofAtom {
                        proof: ProofRef::new("proof-1"),
                        amount_msat: 100,
                    }],
                },
            ],
        );

        assert_eq!(tombstone_first.state().balance(&mint(), &unit()), 0);
        assert!(tombstone_first
            .state()
            .is_token_tombstoned(&WalletEventId::new("token-1")));
        assert!(!tombstone_first
            .state()
            .is_token_live(&WalletEventId::new("token-1")));
    }

    /// Regression: a `Spent` mint-probe verdict is absorbing. A later,
    /// out-of-order `Unspent`/`Unknown` probe for the same proof (e.g. a
    /// stale mint response) must not resurrect it into spendable balance.
    #[test]
    fn mint_probe_spent_verdict_is_absorbing() {
        let mut ledger = WalletLedger::new(16);
        ledger.apply(WalletFact::TokenAdded {
            token_event: WalletEventId::new("token-1"),
            mint: mint(),
            unit: unit(),
            proofs: vec![ProofAtom {
                proof: ProofRef::new("proof-1"),
                amount_msat: 100,
            }],
            via: Provenance::MintRollover,
        });

        ledger.apply(WalletFact::MintProbed {
            proof: ProofRef::new("proof-1"),
            verdict: ProofVerdict::Spent,
        });
        assert_eq!(ledger.state().balance(&mint(), &unit()), 0);

        let summary = ledger.apply(WalletFact::MintProbed {
            proof: ProofRef::new("proof-1"),
            verdict: ProofVerdict::Unspent,
        });

        assert!(!summary.balances_changed);
        assert_eq!(ledger.state().balance(&mint(), &unit()), 0);
        assert_eq!(
            ledger.state().proof_verdict(&ProofRef::new("proof-1")),
            Some(ProofVerdict::Spent)
        );
    }
}
