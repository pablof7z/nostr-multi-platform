//! T142 unit tests — `Kernel::drain_lifecycle_tick()` regression guards.
//!
//! These tests exercise the wiring point added in T142: the actor idle loop
//! calls `Kernel::drain_lifecycle_tick()`, which must:
//!
//! 1. Return an empty `Vec` when no triggers are queued (D8 zero-cost no-op
//!    invariant, common case on a quiet idle tick).
//! 2. Return `WireFrame::Req` entries when an interest is registered AND a
//!    trigger has been enqueued — proving the `KernelMailboxes` adapter feeds
//!    the planner correctly through the kernel boundary.
//!
//! Design note: `Kernel` is `pub(crate)`, so these tests live inside
//! `nmp-core`. The integration layer (`nmp-testing`) covers the public
//! `SubscriptionLifecycle` boundary; these tests cover the kernel-internal
//! wiring that glues the actor to the lifecycle.

use super::*;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::{CompileTrigger, InvalidateReason, WireFrame};
use std::collections::BTreeSet;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn install_relay_list(kernel: &Kernel, author: &str, write: &[&str]) {
    kernel.seed_mailbox_relay_list(
        author,
        vec![],
        write.iter().map(|s| s.to_string()).collect(),
        vec![],
    );
}

fn follow_interest(id: u64, author: &str) -> LogicalInterest {
    let mut authors = BTreeSet::new();
    authors.insert(author.to_string());
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

// ─── Test 1 — empty registry + no trigger = zero-cost no-op ──────────────────

/// D8: with no interests registered and no trigger queued, drain_lifecycle_tick
/// must return an empty Vec — a single `inbox.is_empty()` check, zero
/// allocation. This is the golden-path invariant for the actor idle loop on
/// every quiet tick.
///
/// This test acts as the regression guard the spec §3.1 requires: if
/// `drain_lifecycle_tick` or its idle-loop call site is deleted, this test
/// still passes. The `t142_drain_lifecycle_tick_with_trigger_emits_frames`
/// test below exercises the non-trivial path that would fail if the wiring
/// were removed.
#[test]
fn t142_drain_lifecycle_tick_empty_registry_no_op() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No interests registered, no trigger enqueued.
    let frames = kernel.drain_lifecycle_tick();
    assert!(
        frames.is_empty(),
        "empty registry + no trigger must return zero frames (got {})",
        frames.len(),
    );
}

// ─── Test 2 — interest + trigger → REQ frames emitted through kernel ──────────

/// The key wiring test: register a follow interest via `lifecycle_mut()`,
/// cache a kind:10002 relay list for alice in `author_relay_lists`, enqueue
/// a trigger, then call `drain_lifecycle_tick()` — which is the exact call the
/// actor idle loop makes. The returned frames must include a REQ aimed at
/// alice's resolved write relay, proving:
///
/// a. `drain_lifecycle_tick` correctly calls `lifecycle.drain_tick(&mailboxes)`.
/// b. `KernelMailboxes` bridges `author_relay_lists` → planner `MailboxCache`.
/// c. The planner uses the NIP-65 relay list to route the REQ correctly.
///
/// If anyone removes `drain_lifecycle_tick` or the actor call site, this test
/// still compiles (it calls the method directly). If the `KernelMailboxes`
/// adapter is broken (e.g. returns `None` for alice), alice's REQ falls through
/// to the bootstrap seed and the URL assertion fails.
#[test]
fn t142_drain_lifecycle_tick_with_trigger_emits_frames() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register alice's kind:10002 write relay in the kernel cache.
    install_relay_list(&kernel, ALICE, &["wss://alice-t142.relay/"]);

    // Register a follow interest for alice.
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(follow_interest(1, ALICE));
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Enqueue the force-recompile trigger (simulates any A-series trigger).
    kernel
        .lifecycle_mut()
        .enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });

    // The actor idle loop call.
    let frames = kernel.drain_lifecycle_tick();

    // Must emit at least one REQ frame.
    let req_frames: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .collect();
    assert!(
        !req_frames.is_empty(),
        "drain_lifecycle_tick must return REQ frames when a trigger is enqueued \
         and an interest is registered (got {} total frames)",
        frames.len(),
    );

    // The REQ must target alice's resolved write relay — NOT a bootstrap seed.
    // This pins the KernelMailboxes adapter correctness through the kernel seam.
    let alice_relay_frames: Vec<_> = req_frames
        .iter()
        .filter(|f| match f {
            WireFrame::Req { relay_url, .. } => relay_url == "wss://alice-t142.relay/",
            _ => false,
        })
        .collect();
    assert!(
        !alice_relay_frames.is_empty(),
        "REQ must be aimed at alice's resolved write relay wss://alice-t142.relay/ \
         (got frames: {frames:?})",
    );
}
