//! `WalletFact` — the event-sourced schema the causal trail folds over.
//!
//! Wallet-derived state (proof set / balance) is the fold result of an
//! ordered stream of these facts. Facts differ from the saga
//! (`crate::journal::saga`) by write moment: the saga writes *pre-effect*
//! ("about to consume these inputs"), facts write *post-observation*
//! ("this happened"). Most facts never touch the saga at all — inbound token
//! arrivals, NIP-09 deletes from other devices, and mint-probe reconciliation
//! are all facts with no corresponding saga operation. The one place the two
//! schemas meet is `SagaTransition`, produced from a
//! `crate::journal::saga::WalletSagaEvent` via `From` — never by merging the
//! two representations.

use serde::{Deserialize, Serialize};

use super::saga::{WalletOperationState, WalletSagaEvent};

macro_rules! wallet_fact_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

// A Nostr event id referenced by a fact — a token event (kind:7375) or a
// nutzap event (kind:9321). Deliberately one type for both: a fact only ever
// needs to name *which* event, never to reinterpret its content.
wallet_fact_id!(WalletEventId);
wallet_fact_id!(PubkeyRef);
wallet_fact_id!(MintUrl);
wallet_fact_id!(RelayRef);
wallet_fact_id!(CorrelationId);
wallet_fact_id!(WalletUnit);
// A Cashu proof reference (e.g. a proof's public `C` value or a stable
// derived id) — never the proof secret. Facts name proofs to track spend
// state; they do not carry redemption material.
wallet_fact_id!(ProofRef);

/// One proof's contribution to a token event's amount. Facts carry amounts
/// grouped by proof so a mint-probe verdict on a single proof can update
/// balance without needing the whole token event re-added.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofAtom {
    pub proof: ProofRef,
    pub amount_msat: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProofVerdict {
    Spent,
    Unspent,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Provenance {
    Relay(RelayRef),
    Saga(CorrelationId),
    MintRollover,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeleteCause {
    Nip09Delete { by: WalletEventId },
    LocalRollover { op: CorrelationId },
}

/// One typed fact in the event-sourced stream. Every field here must be one
/// of the id/amount newtypes above (or a nested combination of them) — see
/// `fact_privacy` for the enforcement and rationale. No variant may carry a
/// Cashu proof secret, quote id, NWC secret, plaintext NIP-44 payload, or key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WalletFact {
    TokenAdded {
        token_event: WalletEventId,
        mint: MintUrl,
        unit: WalletUnit,
        proofs: Vec<ProofAtom>,
        via: Provenance,
    },
    TokenDeleted {
        token_event: WalletEventId,
        cause: DeleteCause,
    },
    MintProbed {
        proof: ProofRef,
        verdict: ProofVerdict,
    },
    NutzapRedeemed {
        nutzap: WalletEventId,
        amount_msat: u64,
        sender: PubkeyRef,
    },
    /// Producer wiring from the saga (`WalletSagaEvent` via `From`) — never
    /// constructed by folding wallet events directly.
    SagaTransition {
        op: CorrelationId,
        from: WalletOperationState,
        to: WalletOperationState,
    },
    /// Genesis fact recorded once by `WalletLedger::rebuild_from` after a
    /// restart. Not constructed anywhere else: restart state comes from
    /// folding durable Nostr events plus saga reconciliation, never from
    /// replaying whatever happened to still be in the bounded delta ring.
    StateRebuilt { from: Vec<WalletEventId> },
}

impl From<WalletSagaEvent> for WalletFact {
    fn from(event: WalletSagaEvent) -> Self {
        Self::SagaTransition {
            op: CorrelationId::new(event.op.as_str()),
            from: event.from,
            to: event.to,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::saga::WalletOperationId;

    #[test]
    fn saga_transition_fact_carries_op_as_a_trail_local_correlation_id() {
        let event = WalletSagaEvent {
            op: WalletOperationId::new("op-1"),
            from: WalletOperationState::Draft,
            to: WalletOperationState::Prepared,
        };

        let fact = WalletFact::from(event);
        match fact {
            WalletFact::SagaTransition { op, from, to } => {
                assert_eq!(op.as_str(), "op-1");
                assert_eq!(from, WalletOperationState::Draft);
                assert_eq!(to, WalletOperationState::Prepared);
            }
            other => panic!("expected SagaTransition, got {other:?}"),
        }
    }
}
