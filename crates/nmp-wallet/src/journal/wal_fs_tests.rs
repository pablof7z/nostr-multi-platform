//! Filesystem-specific tests for [`FsWalletWalStore`] (PR-1 of
//! #2910/#2960/#2931): on-disk round-trip, atomic overwrite, corrupt-file
//! skip-not-fatal, per-account directory scoping, and path-segment encoding.

use std::fs;

use super::FsWalletWalStore;
use crate::journal::{
    WalletOperation, WalletOperationId, WalletOperationKind, WalletOperationState, WalletWalStore,
};

fn op(id: &str, state: WalletOperationState) -> WalletOperation {
    WalletOperation::new(
        WalletOperationId::new(id),
        WalletOperationKind::DepositCashu,
        state,
    )
}

#[test]
fn survives_new_instance_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let account = "npub-a";
    {
        let store = FsWalletWalStore::new(dir.path());
        store
            .upsert_operation(account, &op("op1", WalletOperationState::MintPending))
            .unwrap();
    }
    // A fresh store instance on the same dir models a process restart.
    let store = FsWalletWalStore::new(dir.path());
    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id.as_str(), "op1");
    assert_eq!(loaded[0].state, WalletOperationState::MintPending);
}

#[test]
fn upsert_overwrites_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    let account = "npub-a";
    store
        .upsert_operation(account, &op("op1", WalletOperationState::Prepared))
        .unwrap();
    store
        .upsert_operation(account, &op("op1", WalletOperationState::MintPending))
        .unwrap();
    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1, "upsert replaces, never duplicates");
    assert_eq!(loaded[0].state, WalletOperationState::MintPending);
}

/// A single corrupt saga file must be skipped, not fail the whole load (D6 —
/// a bad row must never brick restore). Mirrors `FsPaymentStore`'s discipline.
#[test]
fn corrupt_file_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    let account = "npub-a";
    store
        .upsert_operation(account, &op("good", WalletOperationState::Prepared))
        .unwrap();

    // Hand-write a corrupt `.json` file into the same account directory.
    let account_dir = dir.path().join("wallet_operations").join("npub-a");
    fs::write(account_dir.join("corrupt.json"), b"{ not valid json").unwrap();

    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1, "the corrupt row is skipped, the good one loads");
    assert_eq!(loaded[0].id.as_str(), "good");
}

#[test]
fn payload_round_trip_and_load_is_ignored_by_operation_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    let account = "npub-a";
    let id = WalletOperationId::new("op1");
    store
        .upsert_operation(account, &op("op1", WalletOperationState::Prepared))
        .unwrap();
    store.upsert_payload(account, &id, b"opaque-secret").unwrap();

    // A payload file must never be mistaken for a saga row on load.
    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1);

    assert_eq!(
        store.load_payload(account, &id).unwrap().as_deref(),
        Some(&b"opaque-secret"[..])
    );
    assert!(store.delete_payload(account, &id).unwrap());
    assert_eq!(store.load_payload(account, &id).unwrap(), None);
}

#[test]
fn accounts_get_separate_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    store
        .upsert_operation("npub-a", &op("shared", WalletOperationState::Prepared))
        .unwrap();
    store
        .upsert_operation("npub-b", &op("shared", WalletOperationState::MintPending))
        .unwrap();

    let a = store.load_operations("npub-a").unwrap();
    let b = store.load_operations("npub-b").unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].state, WalletOperationState::Prepared);
    assert_eq!(b[0].state, WalletOperationState::MintPending);
}

#[test]
fn delete_missing_operation_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    store
        .delete_operation("npub-a", &WalletOperationId::new("never-written"))
        .unwrap();
}

#[test]
fn load_from_absent_dir_is_empty_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    assert!(store.load_operations("npub-never-seen").unwrap().is_empty());
}

/// An operation id or account containing path-hostile bytes (a slash, `..`)
/// must be percent-encoded so it can never escape the store directory.
#[test]
fn path_hostile_ids_are_encoded_within_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsWalletWalStore::new(dir.path());
    let account = "npub-a";
    let hostile = "../../etc/passwd";
    store
        .upsert_operation(account, &op(hostile, WalletOperationState::Prepared))
        .unwrap();
    // The row round-trips by its logical id, and nothing was written outside
    // the store's own directory tree.
    let loaded = store.load_operations(account).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id.as_str(), hostile);
    let escaped = dir.path().parent().map(|p| p.join("etc").join("passwd"));
    if let Some(escaped) = escaped {
        assert!(!escaped.exists(), "encoded id must not escape the store dir");
    }
}
