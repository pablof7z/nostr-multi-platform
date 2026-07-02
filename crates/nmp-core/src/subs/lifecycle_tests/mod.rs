//! Subscription-lifecycle test suite, split by behavior area (issue #962).
//!
//! Each submodule below covers one behavior area of `SubscriptionLifecycle`;
//! shared interest/mailbox builders live in `fixtures_tests`. This directory
//! is a descendant of `subs` (same as the former single-file
//! `lifecycle_tests` module), so private-field access (e.g. `l.inbox`,
//! `l.probed_mailboxes`) and the `subs` re-export below both continue to
//! work unchanged.
//!
//! Submodules are named with a `_tests` suffix (not the behavior name
//! alone) so doctrine-lint's `d6::file_is_test_only` filename exemption
//! applies per-file — the `#[cfg(test)]` gate on `mod lifecycle_tests;`
//! lives in `subs/mod.rs`, one hop up, invisible to the line walker when it
//! scans these files directly.
pub(super) use super::*;

mod fixtures_tests;
pub(super) use fixtures_tests::{follow, pubkey, push_legacy};

/// PD-033-C bootstrap content/indexer relay wiring, plus the remaining
/// `lifecycle.rs` setter/accessor round-trips (indexer replace semantics,
/// planner-error seam, probed-mailbox clearing).
mod bootstrap_relays_tests;
/// `lifecycle.rs` constructor + accessor/setter surface: `new`/`Default`
/// zero-state parity and the dead-relay state-machine trigger contract.
mod constructor_and_relay_state_tests;
/// Dead-relay exclusion and recovery: authors route off dead relays and
/// back on once marked alive again; `mark_relay_dead`/`mark_relay_alive`
/// idempotency and trigger emission.
mod dead_relay_exclusion_tests;
/// T142 `drain_tick()` actor-idle-loop driver: empty-inbox no-op, trigger
/// side effects (auth-gate pause/flush), and per-tick compile coalescing.
mod drain_tick_tests;
/// Compile-count smoke tests and the `apply_selection` selection-budget
/// wiring (relay-cap pruning, app-relay preservation, dropped-relay CLOSE
/// emission, indexer-relay override threading).
mod selection_and_apply_tests;
