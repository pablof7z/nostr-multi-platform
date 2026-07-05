//! `nmp-marmot` in-crate tests, split by test-scenario / behavior area.
//!
//! **MDK + NIP-59 round-trip** — publish key package → create group →
//! gift-wrap Welcome → unwrap → join → message round-trip using in-memory
//! storage + explicit keys, driven entirely through the public
//! [`crate::service::MarmotService`] API (the same surface a headless
//! integration-test driver uses).
//!
//! The FULL exit-gate proofs (forward-secrecy, post-compromise, perf) are a
//! separate task in `nmp-testing/tests/marmot_*.rs`; this file proves the
//! crate's public API supports them.
//!
//! ## Submodules
//!
//! - [`fixtures`] — shared in-memory service/actor construction and the
//!   `bootstrap_pair` two-actor group setup every scenario below builds on.
//! - [`round_trip`] — the full create → gift-wrap → join → message round
//!   trip, plus the publish-failure `clear()` recovery path.
//! - [`membership`] — `add_members` / `remove_members` group-size
//!   convergence across peers.
//! - [`leave_and_decline`] — `leave_group` SelfRemove semantics and
//!   `decline_welcome` leaving a group `Inactive`.
//! - [`read_projections`] — `get_groups` / `get_messages` / `group_leaf_map`
//!   reflect real MLS state.
//! - [`key_package_cache`] — `cache_key_package` / `cached_key_packages`
//!   round-trip.
//! - [`error_paths`] — invalid operations surface errors (or, for a
//!   dropped-but-unresolved pending change, self-heal) instead of panicking.
//! - [`orphaned_commit_count`] — V-61: dropped `PendingGroupChange`s are
//!   counted so a host can observe the divergence.
//! - [`init_error_snapshot`] — V-62 / #1651: `MarmotInitError` surfaces in
//!   every `MarmotProjection` snapshot.

mod dispatch_create_group;
mod error_paths;
mod fixtures;
mod init_error_snapshot;
mod key_package_cache;
mod leave_and_decline;
mod membership;
mod orphaned_commit_count;
mod read_projections;
mod round_trip;
