//! T128 integration tests — `PublishQueueEntry` terminal status transitions.
//!
//! T117 wired the kernel's publish path through `PublishEngine` but kept the
//! `PublishQueueEntry.status` pinned at `"accepted_locally"` so the iOS Pulse
//! `ComposeView` wouldn't break. T128 lifts that pin: the engine's terminal
//! verdict (Ok / FailedAfterRetries per-relay, settled when every relay has
//! reached a terminal state) now flips the queue entry to `"ok"` / `"failed"`
//! and carries a per-relay outcome map for the UI.
//!
//! These tests pin the *queue-entry* contract — they snapshot
//! `Kernel::publish_queue_snapshot()` after the relevant engine drive and
//! assert on `status` + `relay_outcomes`. The engine-snapshot side
//! (`recent_ok`, `recent_errors`) is already covered by
//! `publish_engine_tests.rs`; the two contracts are complementary, not
//! redundant. New file (not appended to `publish_engine_tests.rs`) because
//! that file is already 476 LOC and adding ~200 more would breach the 500 LOC
//! hard cap.
//!
//! T-publish-resolver-indexer (codex f81f735): tests updated to seed
//! kind:10002 for each author so `Nip65OutboxResolver` routes via NIP-65
//! rather than the now-removed indexer fallback.
//!
//! Split by behavior area (#962 second wave) into `publish_terminal_status_tests/`:
//!   - `publish_terminal_status_support` — shared fixtures (signed-event
//!     builder, OK-frame payload builder, kind:10002 mailbox seeding,
//!     queue-entry lookup).
//!   - `queue_status_tests` — `publish_queue_snapshot()` status/outcome-map
//!     transitions (T128's core contract).
//!   - `action_results_tests` — `projections.action_results` per-tick drain
//!     (direction review #29).

mod action_results_tests;
mod publish_terminal_status_support;
mod queue_status_tests;
