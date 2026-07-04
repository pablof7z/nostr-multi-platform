//! Backend-level durable-WAL restart + restore tests (PR-1 of
//! #2910/#2960/#2931): `CashuWalletBackend::with_wal_store` +
//! `restore_from_wal` drive the fs-backed WAL end-to-end, and the #2931
//! terminal-`Failed`-redeem-row deletion is proven at the backend seam a
//! re-observed kind:9321 actually hits.

use std::sync::Arc;

use super::*;
use crate::journal::{
    FsWalletWalStore, WalletOperation, WalletOperationKind, WalletOperationState, WalletWalStore,
};

fn fs_store(dir: &std::path::Path) -> Arc<dyn WalletWalStore> {
    Arc::new(FsWalletWalStore::new(dir)) as Arc<dyn WalletWalStore>
}

/// Write operations, drop the whole backend, reconstruct a fresh one over the
/// same on-disk store, restore, and confirm the non-terminal operation is back
/// in the live journal.
#[test]
fn restart_round_trip_rehydrates_pending_operation() {
    let dir = tempfile::tempdir().unwrap();
    let account = "npub-restart";

    {
        let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
        // `restore_from_wal` sets `wal_account`, arming write-through.
        backend.restore_from_wal(account);
        {
            let mut state = lock_state(&backend.state);
            state
                .begin_operation(
                    WalletOperationId::new("deposit-op"),
                    WalletOperationKind::DepositCashu,
                )
                .expect("begin_operation on fresh state");
        }
        // Written through to disk while backend1 is alive.
        assert_eq!(
            backend
                .snapshot(WalletProjectionScope::default())
                .projection
                .pending_operations
                .len(),
            1
        );
    }

    // Process restart: brand-new backend, brand-new store instance, same dir.
    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    // Cold state before restore.
    assert!(backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations
        .is_empty());

    backend.restore_from_wal(account);

    let pending = backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations;
    assert_eq!(pending.len(), 1, "pending operation survived the restart");
    assert_eq!(pending[0].id.as_str(), "deposit-op");
}

/// A settled operation deletes its own row on the terminal transition, so it
/// does not reappear after a restart.
#[test]
fn terminal_operation_does_not_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let account = "npub-terminal";

    {
        let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
        backend.restore_from_wal(account);
        let mut state = lock_state(&backend.state);
        let id = WalletOperationId::new("create-op");
        state
            .begin_operation(id.clone(), WalletOperationKind::CreateCashuWallet)
            .unwrap();
        // Drive it to a terminal state via the allowed path.
        state
            .transition(&id, WalletOperationState::PublishPending)
            .unwrap();
        state
            .transition(&id, WalletOperationState::Settled)
            .unwrap();
    }

    let backend = CashuWalletBackend::with_wal_store(Some(fs_store(dir.path())));
    backend.restore_from_wal(account);
    assert!(backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations
        .is_empty());
}

/// The #2931 fix at the backend seam: a terminal `Failed` redeem row left on
/// disk (e.g. from a crash before its delete committed, or an older build)
/// must NOT be restored into the live journal — otherwise the
/// `DuplicateOperation` guard would block a re-observed kind:9321 forever.
/// After restore, a fresh `begin_operation` for the same id must succeed.
#[test]
fn terminal_failed_redeem_row_does_not_block_reobservation_after_restore() {
    let dir = tempfile::tempdir().unwrap();
    let account = "npub-redeem";
    let store = fs_store(dir.path());

    // Simulate a stuck terminal Failed redeem row sitting on disk.
    let failed = WalletOperation::new(
        WalletOperationId::new("redeem-9321"),
        WalletOperationKind::RedeemNutzap,
        WalletOperationState::Failed,
    );
    store.upsert_operation(account, &failed).unwrap();
    assert_eq!(store.load_operations(account).unwrap().len(), 1);

    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    backend.restore_from_wal(account);

    // Not restored into the live journal.
    assert!(backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations
        .is_empty());
    // Deleted from disk (self-heal).
    assert!(store.load_operations(account).unwrap().is_empty());

    // The load-bearing consequence: a re-observation of the same kind:9321
    // runs begin_operation cleanly, NOT blocked by DuplicateOperation.
    let mut state = lock_state(&backend.state);
    state
        .begin_operation(
            WalletOperationId::new("redeem-9321"),
            WalletOperationKind::RedeemNutzap,
        )
        .expect("re-observation must not be blocked by a stale Failed row");
}

/// Two accounts sharing one backend/store instance must keep their WAL rows
/// isolated across a restore.
#[test]
fn account_switch_does_not_leak_wal_rows() {
    let dir = tempfile::tempdir().unwrap();
    let store = fs_store(dir.path());

    let backend = CashuWalletBackend::with_wal_store(Some(Arc::clone(&store)));
    backend.restore_from_wal("npub-a");
    {
        let mut state = lock_state(&backend.state);
        state
            .begin_operation(
                WalletOperationId::new("op-a"),
                WalletOperationKind::DepositCashu,
            )
            .unwrap();
    }

    // Switch account: reset() clears in-memory state and wal_account; restore
    // rehydrates npub-b (which has no rows).
    backend.reset();
    backend.restore_from_wal("npub-b");
    assert!(backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations
        .is_empty());

    // npub-a's durable row is untouched — a switch back rehydrates it.
    backend.reset();
    backend.restore_from_wal("npub-a");
    let pending = backend
        .snapshot(WalletProjectionScope::default())
        .projection
        .pending_operations;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id.as_str(), "op-a");
}
