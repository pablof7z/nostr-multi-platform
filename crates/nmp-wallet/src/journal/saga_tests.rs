//! Unit tests for [`super::WalletOperation`]/[`super::WalletOperationJournal`]
//! — split out of `saga.rs` (AGENTS.md file-size discipline), mirroring
//! `register.rs`/`runtime.rs`/`selector.rs`'s identical `#[path =
//! "..._tests.rs"]` split.

use super::*;

fn input() -> WalletConsumedInput {
    WalletConsumedInput {
        event_id: "event-1".to_string(),
        mint: "https://mint.example".to_string(),
        unit: "sat".to_string(),
        amount: 21,
    }
}

#[test]
fn value_moving_mint_request_requires_recorded_inputs() {
    let mut operation = WalletOperation::new(
        WalletOperationId::new("op-send"),
        WalletOperationKind::SendNutzap,
        WalletOperationState::Prepared,
    );

    assert_eq!(
        operation.transition(WalletOperationState::MintPending),
        Err(WalletJournalError::MissingConsumedInputs {
            operation_id: "op-send".to_string(),
            kind: WalletOperationKind::SendNutzap,
        })
    );

    operation.record_consumed_input(input());
    assert!(operation
        .transition(WalletOperationState::MintPending)
        .is_ok());
}

#[test]
fn terminal_operations_do_not_transition_again() {
    let mut operation = WalletOperation::new(
        WalletOperationId::new("op-settled"),
        WalletOperationKind::PayBolt11,
        WalletOperationState::Settled,
    );

    assert_eq!(
        operation.transition(WalletOperationState::Failed),
        Err(WalletJournalError::InvalidTransition {
            from: WalletOperationState::Settled,
            to: WalletOperationState::Failed,
        })
    );
}

#[test]
fn journal_lists_only_pending_operations() {
    let mut journal = WalletOperationJournal::new();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("pending"),
            WalletOperationKind::DepositCashu,
            WalletOperationState::MintPending,
        ))
        .unwrap();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("done"),
            WalletOperationKind::DepositCashu,
            WalletOperationState::Settled,
        ))
        .unwrap();

    let pending = journal.pending_operations();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id.as_str(), "pending");
}

#[test]
fn journal_lists_only_terminal_operations_as_the_complement_of_pending() {
    let mut journal = WalletOperationJournal::new();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("pending"),
            WalletOperationKind::DepositCashu,
            WalletOperationState::MintPending,
        ))
        .unwrap();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("settled"),
            WalletOperationKind::DepositCashu,
            WalletOperationState::Settled,
        ))
        .unwrap();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("failed"),
            WalletOperationKind::RedeemNutzap,
            WalletOperationState::Failed,
        ))
        .unwrap();

    let terminal = journal.terminal_operations();
    let mut ids: Vec<&str> = terminal.iter().map(|op| op.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["failed", "settled"]);
}

#[test]
fn journal_transition_emits_a_saga_event_for_the_trail() {
    let mut journal = WalletOperationJournal::new();
    journal
        .insert(WalletOperation::new(
            WalletOperationId::new("op-1"),
            WalletOperationKind::PayBolt11,
            WalletOperationState::Draft,
        ))
        .unwrap();

    let event = journal
        .transition(
            &WalletOperationId::new("op-1"),
            WalletOperationState::Prepared,
        )
        .unwrap();

    assert_eq!(event.op.as_str(), "op-1");
    assert_eq!(event.from, WalletOperationState::Draft);
    assert_eq!(event.to, WalletOperationState::Prepared);
}

#[test]
fn reconciling_an_unknown_operation_can_never_reach_mint_pending_again() {
    // This is the no-double-spend guarantee: restart reconciliation may
    // only move an `Unknown` operation toward publish/settle/fail, never
    // back into a fresh mint request.
    assert!(!WalletOperationState::Unknown.can_transition_to(WalletOperationState::MintPending));
}
