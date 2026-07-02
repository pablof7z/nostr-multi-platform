//! Gap 1: empty interests → empty plan.

use crate::compiler::mailbox::InMemoryMailboxCache;
use crate::compiler::SubscriptionCompiler;

/// An empty interest slice compiles to an empty plan — no `per_relay`
/// entries, no `unroutable_authors`, no panic, and an `Ok` result. The
/// `PlannerError::EmptyInterestSet` variant is defensive-only: an empty
/// input is a valid (empty) plan, NOT an error (see `plan::PlannerError`).
#[test]
fn empty_interests_compile_to_empty_plan() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[])
        .expect("empty input is Ok, not an error");

    assert!(
        plan.per_relay.is_empty(),
        "no relays for an empty interest set"
    );
    assert!(
        plan.unroutable_authors.is_empty(),
        "no authors, so nothing can be unroutable"
    );
    assert!(
        !plan.plan_id.is_empty(),
        "even the empty plan carries a plan-id"
    );
}

/// The empty-input plan-id is deterministic across recompiles — the
/// idempotency check the wire-emitter diff relies on still holds at zero
/// interests.
#[test]
fn empty_interests_plan_id_is_deterministic() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let first = compiler.compile(&[]).expect("compile");
    let second = compiler.compile(&[]).expect("compile");
    assert_eq!(
        first.plan_id, second.plan_id,
        "two compiles of an empty interest set must share a plan-id"
    );
}
