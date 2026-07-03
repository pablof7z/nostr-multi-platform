//! `snapshot()` — the bounded `WalletProjection` never carries a secret or a
//! quote id, mirroring `projection.rs`'s own
//! `projection_never_requires_secret_wallet_material` test.

use super::*;

#[test]
fn capabilities_and_default_snapshot_are_not_configured() {
    let backend = CashuWalletBackend::new();
    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(
        snapshot.projection.readiness,
        WalletReadiness::NotConfigured
    );
    assert!(snapshot.projection.capabilities.create_cashu_wallet);
    assert!(snapshot.projection.capabilities.deposit_cashu);
    assert!(!snapshot.projection.capabilities.pay_bolt11);
    assert!(snapshot.projection.balances.is_empty());
    assert_eq!(snapshot.projection.accepted_mint_count, 0);
}

#[test]
fn snapshot_never_leaks_a_quote_id_proof_or_secret() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.created = true;
        state.mints = vec!["https://testnut.cashu.space".to_string()];
        state.cashu_pubkey_hex = Some("02".to_string() + &"a".repeat(64));
        state.pending_deposits.insert(
            "top-secret-quote-id".to_string(),
            state::PendingDeposit {
                operation_id: crate::journal::WalletOperationId::new("op-1"),
                mint: "https://testnut.cashu.space".to_string(),
                amount_sats: 21,
                minted_proofs: None,
            },
        );
        state.ledger.apply(crate::journal::WalletFact::TokenAdded {
            token_event: crate::journal::WalletEventId::new("token-1"),
            mint: crate::journal::MintUrl::new("https://testnut.cashu.space"),
            unit: crate::journal::WalletUnit::new("sat"),
            proofs: vec![crate::journal::ProofAtom {
                proof: crate::journal::ProofRef::new("proof-secret-marker"),
                amount_msat: 21_000,
            }],
            via: crate::journal::Provenance::MintRollover,
        });
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let json = serde_json::to_string(&snapshot.projection).expect("projection serializes");
    for forbidden in [
        "top-secret-quote-id",
        "proof-secret-marker",
        "secret",
        "nsec",
    ] {
        assert!(
            !json.contains(forbidden),
            "projection JSON leaked forbidden marker {forbidden}: {json}"
        );
    }
    assert_eq!(snapshot.projection.readiness, WalletReadiness::Ready);
    assert_eq!(snapshot.projection.accepted_mint_count, 1);
    assert_eq!(snapshot.projection.balances[0].amount, 21);
}
