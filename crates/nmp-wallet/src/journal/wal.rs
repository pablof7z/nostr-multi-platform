//! Durable pre-publish write-ahead log for the wallet operation journal
//! (PR-1 of #2910/#2960/#2931 — the journal-durability spine).
//!
//! [`WalletOperationJournal`] is at-most-once *in memory*: it records that an
//! operation is about to consume specific inputs before any value-moving mint
//! request goes out. But that record evaporates on process exit — a crash in
//! the window between "operation begun" and "token event published" loses the
//! saga's own memory of the in-flight operation. This module adds the durable
//! shadow of that in-memory journal, mirroring `nmp-nip47`'s `FsPaymentStore`
//! discipline exactly (atomic rename-over-write, one JSON file per record,
//! corrupt files skipped rather than fatal on load, deleted on terminal state).
//!
//! # Two record types, one store
//!
//! 1. **Saga rows** — the serialized [`WalletOperation`] itself. Backend-
//!    agnostic and privacy-clean: only ids, mint URLs, units, and amounts (see
//!    `saga.rs`). Written through on every `begin_operation`/`transition`/
//!    `record_consumed_input`, deleted the moment an operation reaches a
//!    terminal state.
//! 2. **Resume payloads** — an opaque byte blob keyed by operation id, for the
//!    backend-owned, secret-bearing data a resume needs (Cashu proofs etc.)
//!    that must NOT live in the backend-agnostic saga row. PR-1 provides the
//!    store methods only; PR-2 (deposit WAL) and PR-3 (send+redeem WAL) are the
//!    waves that actually write Cashu payloads through them.
//!
//! # Crate-boundary rule
//!
//! This layer never learns a Cashu noun. Saga rows are `WalletOperation`s the
//! journal already owns; payloads are opaque `&[u8]`. Keeping the byte blob
//! opaque here is what lets the durable spine stay in the backend-agnostic
//! `journal` module while Cashu-specific serialization stays in the Cashu
//! backend (PR-2/PR-3).

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{WalletOperation, WalletOperationId, WalletOperationJournal};

/// Failure surface for the WAL store — mirrors `FsPaymentStore`'s
/// `PaymentStoreError` shape (a single opaque backend variant carrying the
/// underlying IO/serde context).
#[derive(Debug)]
pub enum WalError {
    Backend(String),
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(msg) => write!(f, "wallet wal: {msg}"),
        }
    }
}

impl std::error::Error for WalError {}

/// The durable pre-publish WAL seam. Two record families keyed by account
/// pubkey: backend-agnostic saga rows ([`WalletOperation`]) and opaque resume
/// payloads. See the module docs for why the payload is opaque bytes.
pub trait WalletWalStore: Send + Sync {
    /// Insert-or-update the durable saga row for `op` under `account`.
    fn upsert_operation(&self, account: &str, op: &WalletOperation) -> Result<(), WalError>;

    /// Delete the saga row for `id` under `account`. A missing row is success
    /// (idempotent — terminal rows may be deleted more than once).
    fn delete_operation(&self, account: &str, id: &WalletOperationId) -> Result<(), WalError>;

    /// Load every persisted saga row for `account`. A single corrupt record is
    /// skipped, not fatal — a bad row must never brick restore.
    fn load_operations(&self, account: &str) -> Result<Vec<WalletOperation>, WalError>;

    /// Insert-or-update the opaque resume payload for `id` under `account`.
    fn upsert_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
        bytes: &[u8],
    ) -> Result<(), WalError>;

    /// Load the opaque resume payload for `id` under `account`, if any.
    fn load_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
    ) -> Result<Option<Vec<u8>>, WalError>;

    /// Delete the opaque resume payload for `id` under `account`. Returns
    /// whether a payload was actually removed (`false` = none was present —
    /// harmless, e.g. a terminal-state cleanup for an op that never wrote one).
    fn delete_payload(&self, account: &str, id: &WalletOperationId) -> Result<bool, WalError>;
}

/// Load persisted saga rows for `account` back into `journal`, self-healing the
/// store as it goes.
///
/// Non-terminal operations are re-inserted into the live in-memory journal so
/// reconciliation can resume where the crashed process left off. Every terminal
/// row found on disk is deleted instead of re-inserted — this is both a
/// defensive cleanup (a terminal row should normally have been deleted at the
/// transition that made it terminal) AND the #2931 fix:
///
/// A redeem operation left in a terminal `Failed` state must NOT survive back
/// into the live journal. Before this WAL existed, the in-memory journal
/// evaporated on restart, so a re-observation of the same kind:9321 could
/// re-run `begin_operation` and retry the redeem. Making the journal durable
/// would otherwise make behavior strictly worse: the stuck `Failed` row would
/// survive the restart and the `DuplicateOperation` guard would block that
/// re-observation *forever*. Deleting terminal rows on restore preserves the
/// old self-healing property — a subsequent re-observation runs
/// `begin_operation` cleanly. Non-terminal `Unknown` rows are kept (they have a
/// forward reconciliation path per `can_transition_to`); only genuinely
/// terminal (`Settled`/`Failed`) rows are dropped.
pub fn restore_into_journal(
    store: &dyn WalletWalStore,
    account: &str,
    journal: &mut WalletOperationJournal,
) -> Result<(), WalError> {
    let operations = store.load_operations(account)?;
    for op in operations {
        if op.state.is_terminal() {
            let id = op.id.clone();
            store.delete_operation(account, &id)?;
            // Payload delete is a harmless no-op when none was written; PR-2/3
            // are the waves that actually persist payloads.
            store.delete_payload(account, &id)?;
        } else {
            // A duplicate id already live in the journal is skipped, not fatal
            // — restore must never panic on a self-inconsistent store.
            let _ = journal.insert(op);
        }
    }
    Ok(())
}

/// In-memory [`WalletWalStore`] for tests — a store with the same observable
/// contract as [`super::wal_fs::FsWalletWalStore`] but no filesystem. Account
/// scoping is a real nested map so account-leak tests exercise the same keying
/// the fs layout enforces via per-account directories.
#[derive(Default)]
pub struct InMemoryWalletWalStore {
    inner: Mutex<InMemoryInner>,
}

#[derive(Default)]
struct InMemoryInner {
    operations: BTreeMap<String, BTreeMap<String, WalletOperation>>,
    payloads: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

impl InMemoryWalletWalStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, InMemoryInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl WalletWalStore for InMemoryWalletWalStore {
    fn upsert_operation(&self, account: &str, op: &WalletOperation) -> Result<(), WalError> {
        self.lock()
            .operations
            .entry(account.to_string())
            .or_default()
            .insert(op.id.as_str().to_string(), op.clone());
        Ok(())
    }

    fn delete_operation(&self, account: &str, id: &WalletOperationId) -> Result<(), WalError> {
        if let Some(rows) = self.lock().operations.get_mut(account) {
            rows.remove(id.as_str());
        }
        Ok(())
    }

    fn load_operations(&self, account: &str) -> Result<Vec<WalletOperation>, WalError> {
        Ok(self
            .lock()
            .operations
            .get(account)
            .map(|rows| rows.values().cloned().collect())
            .unwrap_or_default())
    }

    fn upsert_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
        bytes: &[u8],
    ) -> Result<(), WalError> {
        self.lock()
            .payloads
            .entry(account.to_string())
            .or_default()
            .insert(id.as_str().to_string(), bytes.to_vec());
        Ok(())
    }

    fn load_payload(
        &self,
        account: &str,
        id: &WalletOperationId,
    ) -> Result<Option<Vec<u8>>, WalError> {
        Ok(self
            .lock()
            .payloads
            .get(account)
            .and_then(|rows| rows.get(id.as_str()).cloned()))
    }

    fn delete_payload(&self, account: &str, id: &WalletOperationId) -> Result<bool, WalError> {
        Ok(self
            .lock()
            .payloads
            .get_mut(account)
            .map(|rows| rows.remove(id.as_str()).is_some())
            .unwrap_or(false))
    }
}

#[cfg(test)]
#[path = "wal_tests.rs"]
mod tests;
