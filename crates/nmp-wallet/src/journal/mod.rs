//! The wallet operation journal: three concerns, distinguished by write
//! moment, that must never collapse into one schema (design doc, "Three
//! Wallet-State Concerns").
//!
//! - [`saga`] — the pre-effect money-safety state machine. Durable,
//!   at-most-once, records consumed inputs before any mint spend.
//! - [`fact`] / [`fact_privacy`] — the event-sourced `WalletFact` schema
//!   facts fold over, and the type-level proof that no fact can carry secret
//!   material.
//! - [`trail`] — the two views over the fact stream: a bounded delta ring
//!   and an unbounded per-atom cause index.
//! - [`ledger`] — the reducer that folds facts into derived state (balances,
//!   proof-set membership) while feeding both trail views.
//! - [`wal`] / [`wal_fs`] — the durable pre-publish write-ahead log: the
//!   filesystem shadow of the in-memory saga journal that survives a process
//!   crash (PR-1 of #2910/#2960/#2931). Backend-agnostic saga rows plus opaque
//!   resume payloads; see the `wal` module docs.
//!
//! The saga is a *producer* into the trail via `WalletSagaEvent` ->
//! `WalletFact::from`; it never depends on `fact`/`trail`/`ledger`. Most
//! facts (inbound token arrivals, other-device NIP-09 deletes, mint-probe
//! reconciliation, incoming nutzaps) never touch the saga at all.

mod fact;
mod fact_privacy;
mod ledger;
mod ledger_state;
mod saga;
mod trail;
mod wal;
mod wal_fs;

pub use fact::{
    CorrelationId, DeleteCause, MintUrl, ProofAtom, ProofRef, ProofVerdict, Provenance, PubkeyRef,
    RelayRef, WalletEventId, WalletFact, WalletUnit,
};
pub use ledger::{
    HistoryFactSeed, WalletApplySummary, WalletBalanceKey, WalletDerivedState, WalletLedger,
};
pub use saga::{
    WalletConsumedInput, WalletJournalError, WalletOperation, WalletOperationId,
    WalletOperationJournal, WalletOperationKind, WalletOperationState, WalletSagaEvent,
};
pub use trail::{WalletCauseIndex, WalletDeltaRing, WalletTrailEntry};
pub use wal::{restore_into_journal, InMemoryWalletWalStore, WalError, WalletWalStore};
pub use wal_fs::FsWalletWalStore;
