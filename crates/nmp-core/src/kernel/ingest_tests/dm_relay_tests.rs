//! `recipient_dm_relays` lookup and the F-02 regression: calling
//! `Kernel::on_dm_relays_changed` enqueues a `CompileTrigger::DmRelayListChanged`
//! trigger so the planner re-routes `PTagRouting::Nip17DmRelays` interests
//! after a kind:10050 fetch closes.
//!
//! The production trigger path is:
//!   `verify_and_persist` → `Kind10050Parser` writes `DmRelayCache` →
//!   wildcard arm snapshots `recipient_dm_relays` before/after →
//!   transition detected → `on_dm_relays_changed` → trigger enqueued.
//!
//! These unit tests exercise `on_dm_relays_changed` directly (the new method
//! added by the F-02 fix) so the contract is locked at the kernel level
//! independently of the parser wiring. The end-to-end path (Kind10050Parser
//! + wildcard arm + trigger fan-out) is covered by the integration test
//! `real_relay_nip17_cold_start_kernel` in `crates/nmp-testing/`.

use super::ingest_support::AUTHOR;
use super::*;

/// `recipient_dm_relays` returns `None` for a pubkey with no kind:10050 — the
/// genuinely-missing case the DM send path treats as not ready.
#[test]
fn recipient_dm_relays_none_for_uncached_pubkey() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        kernel.recipient_dm_relays(AUTHOR).is_none(),
        "a pubkey with no ingested kind:10050 must resolve to None",
    );
}

/// Calling `on_dm_relays_changed` enqueues exactly one
/// `CompileTrigger::DmRelayListChanged` trigger on the lifecycle inbox.
///
/// This is the F-02 regression: a returned `DmRelayListChanged` trigger
/// causes the planner to re-route `PTagRouting::Nip17DmRelays` interests
/// on the next `drain_lifecycle_tick` — the cold-start DM receive path.
#[test]
fn on_dm_relays_changed_enqueues_trigger() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "precondition: no pending triggers"
    );

    kernel.on_dm_relays_changed(AUTHOR, 1_000);

    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "on_dm_relays_changed must enqueue exactly one recompile trigger"
    );
}

/// Two calls for the same author at different timestamps enqueue two
/// triggers (coalescing happens at drain time, not at enqueue time).
#[test]
fn on_dm_relays_changed_two_calls_enqueue_two_triggers() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.on_dm_relays_changed(AUTHOR, 1_000);
    kernel.on_dm_relays_changed(AUTHOR, 2_000);
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        2,
        "two on_dm_relays_changed calls before drain must produce two queued triggers"
    );
}
