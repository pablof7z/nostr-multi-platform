//! `OpfsSqliteStore` inherent impl (open, txn helpers) + the single scoped
//! `unsafe impl Send + Sync` (ADR-0054 §3). The `EventStore` trait impl lives
//! in `nmp-store`, not here. PR-2/PR-3 (#1007).
