//! `TestSupportCommand` — test-only actor verbs (ADR-0065).
//!
//! Grouped under `ActorCommand::TestSupport(TestSupportCommand)`. Dispatch
//! home: `actor/dispatch/cmd_interests.rs` (ingest/GC) +
//! `actor/dispatch/mod.rs` (Barrier).
//!
//! Test-support only (D0: not part of production FFI surface). The whole
//! family is `cfg(any(test, feature = "test-support"))`-gated so the variants
//! never appear in production builds.

use crate::store::VerifiedEvent;

/// Test-support actor commands: pre-verified event ingest (pinned + un-pinned
/// variants), forced GC step, and the deterministic barrier ack.
#[derive(Debug)]
pub enum TestSupportCommand {
    /// Ingest pre-verified timeline events through the test-support kernel
    /// path.
    ///
    /// The caller is responsible for constructing [`VerifiedEvent`] values;
    /// this command routes each through `kernel::ingest_pre_verified_event`
    /// under the `"diag-firehose-stress"` sub-id. It inserts through the
    /// `EventStore`, then updates the lightweight read-cache directly. No
    /// signature re-verification is performed — the `VerifiedEvent` type is
    /// the gate.
    IngestPreVerifiedEvents(Vec<VerifiedEvent>),
    /// Test-support — ingest pre-verified events under a caller-chosen
    /// `sub_id` that does NOT pin the timeline, then ack.
    ///
    /// [`Self::IngestPreVerifiedEvents`] routes every event under the
    /// `"diag-firehose-stress"` sub-id, which pushes each id into
    /// `self.timeline` (`preverified_support.rs` — the `diag-firehose-`
    /// prefix branch). Timeline membership PINS the event against both
    /// RAM-cache and durable-LRU eviction, so that path can never produce an
    /// eviction-eligible corpus. This variant takes the sub-id explicitly:
    /// any sub-id NOT starting with `diag-firehose-` skips the
    /// `timeline.push_back`, leaving the injected events un-pinned and
    /// therefore evictable — the corpus a GC oracle needs.
    ///
    /// Acked: the actor sends `()` on `ack` only after the whole batch is
    /// ingested AND the timeline re-sorted, so the caller can block until the
    /// ingest is SETTLED before triggering GC and measuring deltas. This is
    /// the deterministic replacement for a fixed `sleep` after a
    /// fire-and-forget inject (which returns before the async actor has
    /// processed the batch).
    IngestPreVerifiedEventsForSubId {
        /// Sub-id passed to `Kernel::ingest_pre_verified_event`. Use a value
        /// that does NOT start with `diag-firehose-` (e.g.
        /// `gc-oracle-unpinned`) to keep the injected events un-pinned and
        /// eviction-eligible.
        sub_id: String,
        events: Vec<VerifiedEvent>,
        /// One-shot ack channel, sent after the batch is ingested + re-sorted.
        ack: std::sync::mpsc::SyncSender<()>,
    },
    /// Force one immediate GC pass outside the 60-second tick interval, then
    /// ack.
    ///
    /// Lets test harnesses exercise GC-budget eviction without waiting 60 s
    /// for the wall-clock gate. The pass is identical to the idle-tick GC
    /// path: `Kernel::run_gc_step()` → RAM eviction + store LRU step (if a
    /// budget ceiling is configured via `nmp_app_configure_gc_budget`).
    ///
    /// Acked: the actor sends `()` on `ack` only after `run_gc_step()`
    /// returns, so the cumulative eviction counters
    /// (`PROCESS_RAM_EVENTS_EVICTED`, `PROCESS_STORE_LRU_EVICTED`) reflect a
    /// SETTLED GC pass before the caller reads them. This is the deterministic
    /// replacement for a fixed `sleep` after a fire-and-forget trigger.
    TriggerGcStep {
        /// One-shot ack channel, sent after the GC pass completes.
        ack: std::sync::mpsc::SyncSender<()>,
    },
    /// Test-support synchronisation primitive (V-105). When the actor dequeues
    /// this command it sends `()` on the `ack` channel, proving all prior
    /// commands have been dispatched. Tests that need to wait for the actor to
    /// reach a known state send this after enqueuing the commands they care
    /// about and then block on the ack receiver — deterministic, no blind
    /// `recv_timeout` polling.
    Barrier {
        /// One-shot ack channel. The actor sends `()` here immediately after
        /// processing this command. The sender is `SyncSender` so it can be
        /// `send`-ed without blocking from any thread (the actor never holds a
        /// borrow on the channel after the `send` call).
        ack: std::sync::mpsc::SyncSender<()>,
    },
}
