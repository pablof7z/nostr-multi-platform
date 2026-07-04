//! `CrossMintTransfer` (#3003) — fail-closed gates in `start_cross_mint_transfer`
//! and the source-mint auto-selection + journal pre-record for the happy path.

use super::*;

fn ctx(account_pubkey: Option<&str>) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: 1_700_000_000,
        selected_backend: None,
        account_pubkey,
    }
}

/// A backend already holding proofs at two mints — `target` (the one the
/// transfer is trying to fund) and `other`, which should be auto-selected
/// as the source.
fn backend_with_proofs_at(target: &str, other: &str, other_amount: u64) -> CashuWalletBackend {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![target.to_string()];
        state.proofs.push(state::StoredProof {
            token_event: None,
            mint: other.to_string(),
            proof: synthetic_proof(other_amount, &("02".to_string() + &"bb".repeat(32))),
        });
    }
    backend
}

#[test]
fn no_active_account_fails_closed_without_dispatching() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://source-mint.example",
        100,
    );
    let commands = backend.start_intent(
        ctx(None),
        WalletIntent::CrossMintTransfer {
            target_mint: "https://target-mint.example".to_string(),
            amount_sats: 10,
        },
        Some("cid-1".to_string()),
    );
    assert!(commands
        .iter()
        .all(|c| !matches!(c, ActorCommand::Protocol(_))));
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => assert_eq!(token.code(), ui_codes::NO_ACCOUNT),
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn zero_amount_fails_closed() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://source-mint.example",
        100,
    );
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::CrossMintTransfer {
            target_mint: "https://target-mint.example".to_string(),
            amount_sats: 0,
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::UNSUPPORTED_MINT)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

#[test]
fn malformed_target_mint_fails_closed() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://source-mint.example",
        100,
    );
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::CrossMintTransfer {
            target_mint: "not-a-url".to_string(),
            amount_sats: 10,
        },
        None,
    );
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::UNSUPPORTED_MINT)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

/// The core fail-closed case: no OTHER mint holds enough balance to fund
/// the transfer (even the bare `amount_sats` lower-bound proxy).
#[test]
fn no_fundable_source_mint_fails_closed() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://source-mint.example",
        5, // less than the requested amount below
    );
    let commands = backend.start_intent(
        ctx(Some("aa".repeat(32).as_str())),
        WalletIntent::CrossMintTransfer {
            target_mint: "https://target-mint.example".to_string(),
            amount_sats: 50,
        },
        None,
    );
    assert!(commands
        .iter()
        .all(|c| !matches!(c, ActorCommand::Protocol(_))));
    match &commands[0] {
        ActorCommand::ShowErrorToken { token } => {
            assert_eq!(token.code(), ui_codes::NO_FUNDABLE_SOURCE_MINT)
        }
        other => panic!("expected ShowErrorToken, got {other:?}"),
    }
}

/// Happy-path dispatch: journals `CrossMintTransfer` BEFORE the Protocol
/// command is handed off, and auto-selects the ONLY (and hence largest)
/// non-target mint as the source.
#[test]
fn valid_transfer_journals_before_dispatch_and_picks_source() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://source-mint.example",
        100,
    );
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CrossMintTransfer {
            target_mint: "https://target-mint.example".to_string(),
            amount_sats: 50,
        },
        Some("cid-cross-mint".to_string()),
    );
    assert_eq!(commands.len(), 1);
    let ActorCommand::Protocol(_cmd) = &commands[0] else {
        panic!("expected a Protocol command, got {:?}", commands[0]);
    };

    let state = state::lock_state(&backend.state);
    let op = state
        .journal
        .get(&crate::journal::WalletOperationId::new("cid-cross-mint"))
        .expect("operation recorded before dispatch");
    assert_eq!(op.kind, WalletOperationKind::CrossMintTransfer);
    assert_eq!(op.state, WalletOperationState::Prepared);
}

/// Among several non-target mints, the LARGEST spendable balance is picked
/// as the source — never the first-listed/insertion order.
#[test]
fn picks_the_largest_balance_among_multiple_source_candidates() {
    let backend = backend_with_proofs_at(
        "https://target-mint.example",
        "https://small-source.example",
        20,
    );
    {
        let mut state = state::lock_state(&backend.state);
        state.proofs.push(state::StoredProof {
            token_event: None,
            mint: "https://big-source.example".to_string(),
            proof: synthetic_proof(80, &("02".to_string() + &"cc".repeat(32))),
        });
    }
    let account = "aa".repeat(32);
    let commands = backend.start_intent(
        ctx(Some(&account)),
        WalletIntent::CrossMintTransfer {
            target_mint: "https://target-mint.example".to_string(),
            amount_sats: 50,
        },
        Some("cid-cross-mint-2".to_string()),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::Protocol(_)));
}
