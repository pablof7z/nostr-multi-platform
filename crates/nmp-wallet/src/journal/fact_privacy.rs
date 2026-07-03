//! Field-vocabulary enforcement of `WalletFact`'s privacy invariant.
//!
//! `WalletFact` payloads may carry only event ids, op ids, amounts, proof
//! references (never secrets), and canonical mint/relay URLs — never a Cashu
//! proof secret, quote id, NWC secret, plaintext NIP-44 payload, or key
//! (design doc, "Event-Sourced Reducer And Causal Trail" §4).
//!
//! What this module actually enforces: [`WalletFactSafe`] is a sealed marker
//! trait implemented only for the closed set of *field types* allowed in a
//! `WalletFact` variant. Adding a field of a type outside that set (say, a
//! future `CashuProofSecret`) is a compile error, and the exhaustive match in
//! this module's test forces a reviewer to route every field of every new
//! variant through `assert_field_is_wallet_fact_safe`.
//!
//! What it does **not** enforce: it cannot stop a caller from mis-using an
//! allowed wrapper — nothing rejects `ProofRef::new(a_real_proof_secret)` at
//! compile time, because `ProofRef` is a thin `String` newtype, not a
//! validated-content type. The actual guarantee is narrower and structural:
//! this crate never imports or defines a Cashu proof-secret, quote-id, or
//! NWC-secret type at all, so there is nothing for a caller to *have* in
//! hand to (mis)construct a `ProofRef`/`CorrelationId`/etc. from in the first
//! place. Building a real secret-provenance type (e.g. a mint client's
//! response type that only exposes a public proof reference, never the
//! secret) is protocol-wiring work for a later phase, not this spine.

use super::fact::{
    CorrelationId, DeleteCause, MintUrl, ProofAtom, ProofRef, ProofVerdict, Provenance, PubkeyRef,
    RelayRef, WalletEventId, WalletUnit,
};
use super::saga::WalletOperationState;

mod sealed {
    pub trait Sealed {}
}

/// Marker for types allowed inside a [`WalletFact`](super::fact::WalletFact)
/// field. Implemented only in this module, for exactly the closed vocabulary
/// wallet facts are allowed to name.
pub trait WalletFactSafe: sealed::Sealed {}

macro_rules! impl_wallet_fact_safe {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl sealed::Sealed for $ty {}
            impl WalletFactSafe for $ty {}
        )+
    };
}

impl_wallet_fact_safe!(
    WalletEventId,
    PubkeyRef,
    MintUrl,
    RelayRef,
    CorrelationId,
    WalletUnit,
    ProofRef,
    ProofAtom,
    Provenance,
    DeleteCause,
    ProofVerdict,
    WalletOperationState,
    u64,
);

impl<T: WalletFactSafe> sealed::Sealed for Vec<T> {}
impl<T: WalletFactSafe> WalletFactSafe for Vec<T> {}

#[allow(dead_code)]
fn assert_field_is_wallet_fact_safe<T: WalletFactSafe>(_: &T) {}

#[cfg(test)]
mod tests {
    use super::super::fact::WalletFact;
    use super::*;

    fn sample_facts() -> Vec<WalletFact> {
        vec![
            WalletFact::TokenAdded {
                token_event: WalletEventId::new("token-1"),
                mint: MintUrl::new("https://mint.example"),
                unit: WalletUnit::new("sat"),
                proofs: vec![ProofAtom {
                    proof: ProofRef::new("proof-1"),
                    amount_msat: 21_000,
                }],
                via: Provenance::Relay(RelayRef::new("wss://relay.example")),
            },
            WalletFact::TokenDeleted {
                token_event: WalletEventId::new("token-1"),
                cause: DeleteCause::Nip09Delete {
                    by: WalletEventId::new("delete-1"),
                },
            },
            WalletFact::MintProbed {
                proof: ProofRef::new("proof-1"),
                verdict: ProofVerdict::Spent,
            },
            WalletFact::NutzapRedeemed {
                nutzap: WalletEventId::new("nutzap-1"),
                amount_msat: 5_000,
                sender: PubkeyRef::new("pubkey-1"),
            },
            WalletFact::SagaTransition {
                op: CorrelationId::new("op-1"),
                from: WalletOperationState::MintPending,
                to: WalletOperationState::MintSettled,
            },
            WalletFact::StateRebuilt {
                from: vec![WalletEventId::new("token-1")],
            },
        ]
    }

    /// Exhaustive match (no `..`): every field of every variant must pass
    /// through `assert_field_is_wallet_fact_safe`. Adding a `WalletFact`
    /// variant without extending this match is a compile error, and adding a
    /// field of a type outside the sealed `WalletFactSafe` set is also a
    /// compile error — together these are the type-level enforcement.
    #[test]
    fn every_wallet_fact_field_is_privacy_safe_by_type() {
        for fact in sample_facts() {
            match fact {
                WalletFact::TokenAdded {
                    token_event,
                    mint,
                    unit,
                    proofs,
                    via,
                } => {
                    assert_field_is_wallet_fact_safe(&token_event);
                    assert_field_is_wallet_fact_safe(&mint);
                    assert_field_is_wallet_fact_safe(&unit);
                    assert_field_is_wallet_fact_safe(&proofs);
                    assert_field_is_wallet_fact_safe(&via);
                }
                WalletFact::TokenDeleted { token_event, cause } => {
                    assert_field_is_wallet_fact_safe(&token_event);
                    assert_field_is_wallet_fact_safe(&cause);
                }
                WalletFact::MintProbed { proof, verdict } => {
                    assert_field_is_wallet_fact_safe(&proof);
                    assert_field_is_wallet_fact_safe(&verdict);
                }
                WalletFact::NutzapRedeemed {
                    nutzap,
                    amount_msat,
                    sender,
                } => {
                    assert_field_is_wallet_fact_safe(&nutzap);
                    assert_field_is_wallet_fact_safe(&amount_msat);
                    assert_field_is_wallet_fact_safe(&sender);
                }
                WalletFact::SagaTransition { op, from, to } => {
                    assert_field_is_wallet_fact_safe(&op);
                    assert_field_is_wallet_fact_safe(&from);
                    assert_field_is_wallet_fact_safe(&to);
                }
                WalletFact::StateRebuilt { from } => {
                    assert_field_is_wallet_fact_safe(&from);
                }
            }
        }
    }

    /// Second, weaker smoke test at the serde boundary: even though
    /// `ProofRef` legitimately serializes a field named `"proof"`, no fact
    /// should ever serialize a secret-shaped marker. This catches a future
    /// field rename or an accidental `Debug`-derived leak the type-level test
    /// above cannot see (serde behavior is not part of the type system).
    #[test]
    fn wallet_fact_serialization_never_contains_secret_markers() {
        for fact in sample_facts() {
            let json = serde_json::to_string(&fact).expect("fact serializes");
            for forbidden in [
                "secret",
                "quote_id",
                "nsec",
                "plaintext",
                "private_key",
                "bearer",
            ] {
                assert!(
                    !json.to_lowercase().contains(forbidden),
                    "WalletFact JSON leaked forbidden marker {forbidden}: {json}"
                );
            }
        }
    }
}
