//! D2 regression pin — `PlanCoverageHook` seam mechanics.
//!
//! D2 doctrine: "negentropy reconciliation before REQ subscriptions". The
//! kernel-side enabler for that is the [`crate::subs::PlanCoverageHook`] seam:
//! `nmp-nip77`'s `apply_coverage_filter` is installed via
//! [`SubscriptionLifecycle::set_coverage_hook`] so it can rewrite the
//! `CompiledPlan` (drop authoritative pairs, bump `since`) **after** the M2
//! compiler produces the plan but **before** `plan_diff` emits the wire
//! frames.
//!
//! These tests pin that seam *independently of `nmp-nip77`*. They install a
//! stub closure (no NIP-77 dependency, no D0 violation) and assert:
//!
//! 1. The hook fires exactly once per `recompile_and_diff`.
//! 2. The hook fires AT a position where it sees a fully-compiled plan
//!    (`per_relay` populated by the M2 compiler) — i.e. *after* `compile()`.
//! 3. A mutation the hook performs reaches the wire diff — i.e. the hook runs
//!    *before* `plan_diff`.
//! 4. With no hook installed the plan flows through unchanged (the kernel-only
//!    path must link and behave cleanly without any NIP-77 dependency).
//!
//! Why a kernel-internal pin when `nmp-testing/tests/framework_magic_c10.rs`
//! already exercises the seam end-to-end? That integration test wires the
//! *real* `apply_coverage_filter` and would silently lapse the moment the
//! seam's position drifts but `nmp-nip77` is still present. This pin has zero
//! `nmp-nip77` coupling, so it survives independently and fails loudly if the
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
    assert_eq!(req_count(&frames2), 0, "no new REQ once the plan is dropped");
}

// ─── 3) No hook installed → kernel-only path is unchanged ────────────────────

/// The default (kernel-only) path: with no coverage hook installed the plan
/// flows through `recompile_and_diff` unchanged. This guards the
/// `coverage_hook: None` default that lets `nmp-core` link without any
/// `nmp-nip77` dependency (D0).
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

// ─── 4) D2 production-wiring sentinel ────────────────────────────────────────

/// Sentinel for the open D2 gap surfaced by the 2026-05-20 audit.
///
/// The `PlanCoverageHook` seam (pinned by the tests above) is mechanically
/// sound, but the *production* kernel — `actor::run_actor` and the
/// `nmp-core/src/ffi` app surface — never calls `set_coverage_hook`. The only
/// real wiring lives in `nmp-testing/tests/framework_magic_c10.rs`. The
/// shipping kernel therefore does NOT enforce D2's "negentropy before REQ":
/// every plan flows straight to raw REQ.
///
/// This is a systemic state, not an isolated miss — the sibling
/// `set_watermark_fn` seam is *also* only wired in tests (`replay_tests.rs`,
/// `since_rewrite_tests.rs`). Both are kernel seams awaiting an app-layer
/// assembly step that installs them at startup.
///
/// The wiring cannot land here: `nmp-core` must not depend on `nmp-nip77`
/// (D0 — kernel grows no app nouns; would also be a dependency cycle). It
/// belongs in whatever crate assembles the kernel for the shell. This test is
/// `#[ignore]`d on purpose: un-ignore it (and replace the body with a real
/// assertion that the assembled kernel has a coverage hook installed) once
/// that assembly step exists. See `TODO(D2)` in `subs/mod.rs`.
#[test]
#[ignore = "D2 gap: production kernel does not install a coverage hook yet — \
            see TODO(D2) in subs/mod.rs. Un-ignore when the app-layer kernel \
            assembly installs nmp_nip77::apply_coverage_filter at startup."]
fn d2_production_kernel_installs_coverage_hook() {
    panic!(
        "D2 not enforced in production: no set_coverage_hook call exists in \
         actor::run_actor or the nmp-core FFI surface. apply_coverage_filter \
         is only wired in nmp-testing's framework_magic_c10 integration test."
    );
}
