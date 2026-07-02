//! Unit tests for `MemEventStore` — P2 invariant checks.
//!
//! Integration tests using the full `StoreHarness` live in
//! `crates/nmp-testing/tests/store_*.rs`.
//!
//! Sub-modules (split to stay under the 500-LOC file-size hard cap):
//!   insert_tests       — tombstone max-merge and replaceable-dup provenance
//!   query_visit_tests  — early-stop visitor and `query` wrapper ordering
//!   authors_kind_tests — `StoreQuery::AuthorsKind` multi-author query invariants
//!   relay_index_tests  — V-52 relay-origin reverse-index invariants
//!   relay_kind_tests   — #1518 relay×kind presence-index invariants

mod authors_kind_tests;
mod insert_tests;
mod query_visit_tests;
mod relay_index_tests;
mod relay_kind_tests;
// #1811 — in-memory full-text-search backend tests.
mod fts_tests;

mod ingest_log_tests;
// Fix-verification tests (split for 500-LOC cap).
mod ingest_log_fix_tests;
// ADR-0072 §6 step-4 — Protected-cursor log-retention trim tests.
mod retention_tests;
