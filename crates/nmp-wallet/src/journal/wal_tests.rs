//! Unit tests for the durable pre-publish WAL contract (PR-1 of
//! #2910/#2960/#2931), exercised against [`InMemoryWalletWalStore`] and the
//! backend-agnostic [`restore_into_journal`] restore/self-heal logic.

use super::{restore_into_journal, InMemoryWalletWalStore, WalletWalStore};
use crate::journal::{
    WalletOperation, WalletOperationId, WalletOperationJournal, WalletOperationKind,
    WalletOperationState,
};

fn op(id: &str, kind: WalletOperationKind, state: WalletOperationState) -> WalletOperation {
    WalletOperation::new(WalletOperationId::new(id), kind, state)
}

#[test]
fn upsert_load_delete_round_trip() {
    let store = InMemoryWalletWalStore::new();
    let account = "acct-a";
    store
        .upsert_operation(
            account,
            &op(
                "op1",
                WalletOperationKind::DepositCashu,
                WalletOperationState::Prepared,
            ),
        )
        .unwrap();
    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id.as_str(), "op1");

    store
        .delete_operation(account, &WalletOperationId::new("op1"))
        .unwrap();
    assert!(store.load_operations(account).unwrap().is_empty());
    // Deleting a missing row is idempotent success.
    store
        .delete_operation(account, &WalletOperationId::new("op1"))
        .unwrap();
}

#[test]
fn payload_round_trip_is_opaque_bytes() {
    let store = InMemoryWalletWalStore::new();
    let account = "acct-a";
    let id = WalletOperationId::new("op1");
    assert_eq!(store.load_payload(account, &id).unwrap(), None);
    store.upsert_payload(account, &id, b"secret-bytes").unwrap();
    assert_eq!(
        store.load_payload(account, &id).unwrap().as_deref(),
        Some(&b"secret-bytes"[..])
    );
    assert!(store.delete_payload(account, &id).unwrap());
    // Second delete: nothing there, returns false, not an error.
    assert!(!store.delete_payload(account, &id).unwrap());
    assert_eq!(store.load_payload(account, &id).unwrap(), None);
}

/// Two accounts' WAL entries must never leak into each other — the fs layout
/// enforces this with per-account directories; the in-memory store mirrors it
/// with a nested map keyed by account.
#[test]
fn account_scoping_is_isolated() {
    let store = InMemoryWalletWalStore::new();
    store
        .upsert_operation(
            "acct-a",
            &op(
                "shared-id",
                WalletOperationKind::DepositCashu,
                WalletOperationState::Prepared,
            ),
        )
        .unwrap();
    store
        .upsert_payload("acct-a", &WalletOperationId::new("shared-id"), b"a-bytes")
        .unwrap();

    // Same op id, different account: must be invisible from acct-b.
    assert!(store.load_operations("acct-b").unwrap().is_empty());
    assert_eq!(
        store
            .load_payload("acct-b", &WalletOperationId::new("shared-id"))
            .unwrap(),
        None
    );
    // acct-a still sees its own row/payload.
    assert_eq!(store.load_operations("acct-a").unwrap().len(), 1);
}

#[test]
fn restore_reinserts_non_terminal_operations() {
    let store = InMemoryWalletWalStore::new();
    let account = "acct-a";
    store
        .upsert_operation(
            account,
            &op(
                "pending-op",
                WalletOperationKind::DepositCashu,
                WalletOperationState::MintPending,
            ),
        )
        .unwrap();

    let mut journal = WalletOperationJournal::new();
    restore_into_journal(&store, account, &mut journal).unwrap();

    let pending = journal.pending_operations();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id.as_str(), "pending-op");
    // Non-terminal row is kept on disk (still in-flight, still needs durability).
    assert_eq!(store.load_operations(account).unwrap().len(), 1);
}

/// The #2931 fix, tested at the backend-agnostic layer: a terminal `Failed`
/// redeem row must NOT survive restore back into the live journal, and it must
/// be deleted from the store so a subsequent re-observation of the same
/// operation id can run `begin_operation` again without tripping
/// `DuplicateOperation`.
#[test]
fn restore_deletes_terminal_failed_redeem_and_does_not_block_reobservation() {
    let store = InMemoryWalletWalStore::new();
    let account = "acct-a";
    let failed_redeem = op(
        "redeem-9321",
        WalletOperationKind::RedeemNutzap,
        WalletOperationState::Failed,
    );
    store.upsert_operation(account, &failed_redeem).unwrap();

    let mut journal = WalletOperationJournal::new();
    restore_into_journal(&store, account, &mut journal).unwrap();

    // Not re-inserted into the live journal (neither pending nor terminal view).
    assert!(journal.pending_operations().is_empty());
    assert!(journal.terminal_operations().is_empty());
    assert!(journal.get(&WalletOperationId::new("redeem-9321")).is_none());

    // Deleted from disk (self-heal), so it can't block a future restore either.
    assert!(store.load_operations(account).unwrap().is_empty());

    // The load-bearing consequence: a re-observation re-runs begin_operation
    // (modeled here as a fresh insert of the same id) and is NOT blocked.
    journal
        .insert(op(
            "redeem-9321",
            WalletOperationKind::RedeemNutzap,
            WalletOperationState::Draft,
        ))
        .expect("re-observation of the same kind:9321 must not be blocked");
}

/// A terminal `Settled` row is likewise dropped on restore (defensive
/// self-heal — it should normally have been deleted at the settling
/// transition, but a WAL must tolerate finding one).
#[test]
fn restore_deletes_terminal_settled_row() {
    let store = InMemoryWalletWalStore::new();
    let account = "acct-a";
    store
        .upsert_operation(
            account,
            &op(
                "settled-op",
                WalletOperationKind::DepositCashu,
                WalletOperationState::Settled,
            ),
        )
        .unwrap();

    let mut journal = WalletOperationJournal::new();
    restore_into_journal(&store, account, &mut journal).unwrap();

    assert!(journal.terminal_operations().is_empty());
    assert!(store.load_operations(account).unwrap().is_empty());
}
