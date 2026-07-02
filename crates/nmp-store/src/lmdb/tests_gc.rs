//! V-117 / V-118 GC regression tests for the LMDB backend.
//!
//! Split by behavior area (500-LOC hard cap) into `tests_gc/`:
//!
//!   * `budget_bounds` — `gc_step` duration / count / eviction budget
//!     enforcement (V-117 part A: bounded Phase-1 scan, O(1) Phase-2 count,
//!     finite-ceiling LRU eviction).
//!   * `pass_state` — state carried between successive `gc_step` calls on the
//!     same store (V-118 expiry-index cursor advance across passes; the
//!     tombstone-purge gate that suppresses redundant scans within the same
//!     purge interval).
//!   * `v118_expiry_index` — expiration-index correctness and
//!     backfill-on-reopen (closes #1097): same-`created_at` blocks never
//!     stall older expired events, index temporal ordering, backfill of
//!     pre-index stores, and the backfill migration gate's stability across
//!     reopens.
//!
//! Test 10 (bulk-delete expiry-index cleanup) lives in
//! `tests_gc_bulk_delete.rs` (split earlier, also for the 500-LOC cap).

#![cfg(feature = "lmdb-backend")]

mod budget_bounds;
mod pass_state;
mod v118_expiry_index;
