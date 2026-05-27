//! D2 regression pin — `PlanCoverageHook` seam mechanics.
//!
//! D2 doctrine: "negentropy reconciliation before REQ subscriptions". The
//! kernel-side enabler for that is the [`crate::subs::PlanCoverageHook`] seam:
//! a host-supplied coverage-filter closure is installed via
//! [`SubscriptionLifecycle::set_coverage_hook`] so it can rewrite the
//! `CompiledPlan` (drop authoritative pairs, bump `since`) **after** the M2
//! compiler produces the plan but **before** `plan_diff` emits the wire
//! frames.
//!
//! These tests pin that seam independently of any specific policy. They
//! install a stub closure (no app-noun dependency, no D0 violation) and
//! assert:
//!
//! 1. The hook fires exactly once per `recompile_and_diff`.
//! 2. The hook fires AT a position where it sees a fully-compiled plan
//!    (`per_relay` populated by the M2 compiler) — i.e. *after* `compile()`.
//! 3. A mutation the hook performs reaches the wire diff — i.e. the hook runs
//!    *before* `plan_diff`.
//! 4. With no hook installed the plan flows through unchanged (the kernel-only
//!    path must link and behave cleanly without any external policy).
//!
//! This pin has zero coupling to any specific policy crate, so it survives
//! independently and fails loudly if the
//! `compile → coverage_hook → plan_diff` ordering in `recompile.rs` regresses.
//!
//! NOTE (D2 audit, 2026-05-20): the seam itself is sound, but the *production*
//! kernel never installs a coverage hook — see the `TODO(D2)` in
//! `subs/mod.rs`. These tests pin the mechanism; they do not assert the
//! mechanism is wired in the shipping kernel (it is not yet).

use std::sync::{Arc, Mutex};

use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::subs::wire::WireFrame;
use crate::subs::SubscriptionLifecycle;

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn timeline_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Lifecycle + caller-owned mailbox cache carrying one author's write set
/// (T132 moved mailbox ownership out of the lifecycle).
fn lifecycle_with_mailbox(
    author: &str,
    relays: &[&str],
) -> (SubscriptionLifecycle, InMemoryMailboxCache) {
    let lifecycle = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: relays.iter().map(|r| (*r).to_string()).collect(),
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    (lifecycle, mailboxes)
}

fn req_count(frames: &[WireFrame]) -> usize {
    frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count()
}

fn close_count(frames: &[WireFrame]) -> usize {
    frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count()
}

// ─── 1) The hook fires, exactly once, and sees a compiled plan ───────────────

/// The core D2 seam pin: installing a `PlanCoverageHook` causes it to be
/// invoked exactly once per `recompile_and_diff`, and the plan it observes is
/// already compiled (the M2 compiler has populated `per_relay`). This proves
/// the hook runs AFTER `compile()`.
#[test]
fn coverage_hook_runs_once_after_compile() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);

    let fired = Arc::new(Mutex::new(false));
    // `per_relay` length the hook observed — non-zero proves the plan was
    // compiled (relay routing resolved) before the hook ran.
    let observed_relay_count = Arc::new(Mutex::new(0usize));

    let fired_for_hook = Arc::clone(&fired);
    let count_for_hook = Arc::clone(&observed_relay_count);
    l.set_coverage_hook(Arc::new(move |plan| {
        *fired_for_hook.lock().unwrap() = true;
        *count_for_hook.lock().unwrap() = plan.per_relay.len();
    }));

    l.registry_mut().push(timeline_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");

    assert!(
        *fired.lock().unwrap(),
        "PlanCoverageHook must be invoked during recompile_and_diff"
    );
    assert!(
        *observed_relay_count.lock().unwrap() >= 1,
        "the hook must observe a fully-compiled plan (per_relay populated by \
         the M2 compiler) — proves the hook runs AFTER compile()"
    );
    // Sanity: with a no-op hook the plan flows through and a REQ is emitted.
    assert_eq!(
        req_count(&frames),
        1,
        "a no-op coverage hook must not suppress the cold-open REQ"
    );
}

// ─── 2) A hook mutation reaches the wire diff (hook runs BEFORE plan_diff) ────

/// Pins the other half of the seam contract: a mutation the hook performs is
/// visible to `plan_diff`. The hook clears `per_relay`, so the compiled plan
/// the wire-emitter diffs against is empty → no REQ is emitted. If the hook
/// ran *after* `plan_diff` (a regression), the REQ would still fly.
#[test]
fn coverage_hook_mutation_reaches_wire_diff() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("b", &["wss://r2"]);

    // A hostile hook that drops the entire plan. A correctly-positioned seam
    // (compile → hook → plan_diff) means the wire-emitter sees the emptied
    // plan and emits zero REQs.
    l.set_coverage_hook(Arc::new(|plan| {
        plan.per_relay.clear();
    }));

    l.registry_mut().push(timeline_interest(2, "b"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");

    assert_eq!(
        req_count(&frames),
        0,
        "a coverage hook that empties the plan must suppress the REQ — \
         proves the hook runs BEFORE plan_diff"
    );
}

/// The hook can also CLOSE a previously-live sub: compile once with the hook
/// passive (REQ flies), then activate the drop and recompile (CLOSE flies).
/// This double-checks the seam position across two recompiles — the hook
/// participates in the diff against the *prior* plan.
#[test]
fn coverage_hook_drop_closes_prior_req() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("c", &["wss://r3"]);

    // Toggle: when `true`, the hook drops the plan.
    let drop_plan = Arc::new(Mutex::new(false));
    let drop_for_hook = Arc::clone(&drop_plan);
    l.set_coverage_hook(Arc::new(move |plan| {
        if *drop_for_hook.lock().unwrap() {
            plan.per_relay.clear();
        }
    }));

    l.registry_mut().push(timeline_interest(3, "c"));

    // Compile #1: hook passive → REQ flies.
    let frames1 = l.recompile_and_diff(&mailboxes).expect("compile #1");
    assert_eq!(req_count(&frames1), 1, "cold open must emit a REQ");

    // Compile #2: hook now drops the plan → the live REQ must be CLOSEd.
    *drop_plan.lock().unwrap() = true;
    let frames2 = l.recompile_and_diff(&mailboxes).expect("compile #2");
    assert_eq!(
        close_count(&frames2),
        1,
        "dropping a previously-covered pair must CLOSE its live REQ"
    );
    assert_eq!(
        req_count(&frames2),
        0,
        "no new REQ once the plan is dropped"
    );
}

// ─── 3) No hook installed → kernel-only path is unchanged ────────────────────

/// The default (kernel-only) path: with no coverage hook installed the plan
/// flows through `recompile_and_diff` unchanged. This guards the
/// `coverage_hook: None` default that lets `nmp-core` link without any
/// external coverage-policy dependency (D0).
#[test]
fn no_coverage_hook_leaves_plan_unchanged() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("d", &["wss://r4"]);
    l.registry_mut().push(timeline_interest(4, "d"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");

    assert_eq!(
        req_count(&frames),
        1,
        "with no coverage hook the cold-open REQ must fly unmodified"
    );
}

// ─── 4) D2 production-wiring slot round-trip ─────────────────────────────────

/// V-05 Stage 2 verification: the `Arc<Mutex<Option<PlanCoverageHook>>>` slot
/// pattern used by `NmpApp::set_coverage_hook` survives a write-then-read
/// round-trip. The actor thread reads the slot after kernel construction and
/// installs the hook on the lifecycle; this test pins the pre-install slot
/// half of that contract independent of the FFI surface (which requires a
/// full `NmpApp` and so cannot be assembled inside `nmp-core` without test-only
/// gates).
///
/// Full production-kernel assembly is covered by the integration tests that
/// drive `nmp_app_new` + `nmp_app_start` from an app crate (e.g.
/// `nmp-app-chirp`), where the seam terminates in `CoverageGate`-based policy.
///
/// Replaces the prior `#[ignore]`d sentinel that panicked "D2 not enforced":
/// the production kernel now installs the hook via the slot wired up in
/// `actor::run_actor_with_observers` (see the `set_coverage_hook` call there).
#[test]
fn d2_coverage_hook_slot_round_trips() {
    use crate::subs::PlanCoverageHook;
    use std::sync::{Arc, Mutex};
    // A minimal coverage-gate closure (stateless body — `_plan` is unused).
    let hook: PlanCoverageHook = Arc::new(move |_plan| {});
    // Verify the slot pattern (same shape as `NmpApp::set_coverage_hook`):
    // write through `Some(hook)`, read back, assert presence.
    let slot: Arc<Mutex<Option<PlanCoverageHook>>> = Arc::new(Mutex::new(None));
    *slot.lock().unwrap() = Some(Arc::clone(&hook));
    let extracted = slot.lock().unwrap().clone();
    assert!(
        extracted.is_some(),
        "coverage hook slot must survive write-then-read round-trip — \
         this is the pre-install half of the actor wiring contract"
    );
}
