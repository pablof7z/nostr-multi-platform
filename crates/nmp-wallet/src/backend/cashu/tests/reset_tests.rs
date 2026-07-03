//! `CashuWalletBackend::reset` — the cross-account data-leak fix (epic #2864
//! Wave C, #2908): resets must fully clear the in-memory state, not just the
//! `created` flag, since `mints`/`cashu_pubkey_hex`/balances/pending
//! operations are all identity-scoped material a subsequent account must
//! never see.

use super::*;

#[test]
fn reset_clears_created_flag_mints_and_pubkey() {
    let backend = backend_with_mint();
    {
        let mut state = lock_state(&backend.state);
        state.created = true;
        state.cashu_pubkey_hex = Some("02".repeat(33));
    }

    backend.reset();

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(
        snapshot.projection.readiness,
        WalletReadiness::NotConfigured
    );
    assert_eq!(snapshot.projection.cashu_p2pk_pubkey, None);
    assert_eq!(snapshot.projection.accepted_mint_count, 0);
}

#[test]
fn reset_clears_pending_operations_and_balances() {
    let backend = backend_with_mint();
    {
        let mut state = lock_state(&backend.state);
        state.created = true;
        state
            .begin_operation(
                WalletOperationId::new("op-before-reset"),
                WalletOperationKind::CreateCashuWallet,
            )
            .expect("begin_operation succeeds on fresh state");
    }
    assert!(!backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations
        .is_empty());

    backend.reset();

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert!(snapshot.projection.pending_operations.is_empty());
    assert!(snapshot.projection.balances.is_empty());
}

/// A second `reset()` (e.g. two logouts in a row, or logout with no prior
/// account) must stay a harmless no-op rather than panic.
#[test]
fn reset_on_a_never_created_wallet_is_a_harmless_no_op() {
    let backend = CashuWalletBackend::new();
    backend.reset();
    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(
        snapshot.projection.readiness,
        WalletReadiness::NotConfigured
    );
}
