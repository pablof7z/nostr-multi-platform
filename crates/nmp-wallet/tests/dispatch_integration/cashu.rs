//! `nmp.wallet.cashu.*` registry-level dispatch tests — split out of
//! `dispatch_integration.rs` (AGENTS.md LOC discipline). Shares the same
//! `registry_with_wallet_actions`/`dispatch_ok` harness via `use super::*;`.

use super::*;

// --- cashu.create ---------------------------------------------------------------

#[test]
fn cashu_create_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuCreateAction {
        mint: "https://mint.example.com".to_string(),
    }
    .encode();
    let sent = dispatch_ok(&registry, CASHU_CREATE, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.create must enqueue at least one ActorCommand"
    );
}

// --- cashu.recover ----------------------------------------------------------------

/// `cashu.recover` is reachable BY NAME (the payload decodes, `start()` and
/// `execute()` both run) and dispatches a real command (#2965:
/// `CashuWalletBackend` now implements `RecoverCashuWallet` —
/// `recover::RecoverCashuWalletCommand`). Before this change, the namespace
/// was unreachable at the byte doorway at all (`NotTypedCapable`); before
/// #2965, `start()` rejected unconditionally regardless of what backend was
/// registered.
#[test]
fn cashu_recover_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuRecoverAction {}.encode();
    let sent = dispatch_ok(&registry, CASHU_RECOVER, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.recover must enqueue at least one ActorCommand"
    );
}

// --- cashu.set_mints --------------------------------------------------------------

/// `cashu.set_mints` is reachable BY NAME. The registered backend has no
/// created wallet, so it fails closed with `NO_CASHU_WALLET` at the backend
/// layer — but `start_bytes`/`execute_bytes` both still succeed (the typed
/// payload decodes, `start()`'s own non-empty/well-formed gates pass, and
/// `execute()` reaches the backend), proving the doorway itself works
/// end-to-end; the fail-closed backend precondition is `set_mints_tests.rs`'s
/// job at the unit level.
#[test]
fn cashu_set_mints_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuSetMintsAction {
        mints: vec!["https://mint.example.com".to_string()],
    }
    .encode();
    let sent = dispatch_ok(&registry, CASHU_SET_MINTS, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.set_mints must enqueue at least one ActorCommand"
    );
}

// --- cashu.cross_mint_transfer (#3003) ---------------------------------------

/// `cashu.cross_mint_transfer` is reachable BY NAME at the byte doorway: a
/// well-formed payload decodes, `start()`'s capability/shape gates pass, and
/// `execute()` dispatches through the selector. No proofs are seeded at any
/// mint in this registry-level test, so the dispatched `CrossMintTransfer`
/// intent itself fails closed inside the backend
/// (`NO_FUNDABLE_SOURCE_MINT`) — this test only proves the namespace is
/// wired end-to-end, not the fundability path (covered by
/// `backend::cashu::tests::cross_mint_tests`).
#[test]
fn cashu_cross_mint_transfer_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuCrossMintTransferAction {
        target_mint: "https://target-mint.example".to_string(),
        amount_sats: 21,
    }
    .encode();
    let sent = dispatch_ok(&registry, CASHU_CROSS_MINT_TRANSFER, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.cross_mint_transfer must enqueue at least one ActorCommand"
    );
}

// --- cashu.deposit_quote -----------------------------------------------------------

#[test]
fn cashu_deposit_quote_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuDepositQuoteAction {
        mint: "https://mint.example.com".to_string(),
        amount_sats: 21_000,
    }
    .encode();
    let sent = dispatch_ok(&registry, CASHU_DEPOSIT_QUOTE, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.deposit_quote must enqueue at least one ActorCommand"
    );
}

// --- cashu.complete_deposit --------------------------------------------------------

#[test]
fn cashu_complete_deposit_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = CashuCompleteDepositAction {
        quote_id: "quote-abc-123".to_string(),
    }
    .encode();
    let sent = dispatch_ok(&registry, CASHU_COMPLETE_DEPOSIT, &bytes);
    assert!(
        !sent.is_empty(),
        "cashu.complete_deposit must enqueue at least one ActorCommand"
    );
}
