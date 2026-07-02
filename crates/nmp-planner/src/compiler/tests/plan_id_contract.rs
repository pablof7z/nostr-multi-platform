//! Gap 6 + Gap 7: the `compile()` vs `compile_with_context()` plan-id
//! contract, and which inputs `compute_plan_id` does (and does not) cover.

use super::{author_interest, pk, write_snapshot};
use crate::compiler::mailbox::InMemoryMailboxCache;
use crate::compiler::{CompileContext, SubscriptionCompiler};
use crate::interest::InterestLifecycle;

// ── Gap 6: compile() vs compile_with_context() plan-id contract ─────────

/// `compile()` pins the `CompileContext` to its default (both version
/// counters at 0). Two `compile_with_context` calls with DIFFERENT
/// contexts must produce different plan-ids for the same interests — the
/// stability contract the doc-comment on `compile()` warns about.
#[test]
fn compile_with_context_plan_id_tracks_the_context() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);
    let interests = [author_interest(
        1,
        &["alice"],
        &[1],
        InterestLifecycle::Tailing,
    )];

    let v0 = compiler
        .compile_with_context(&interests, &CompileContext::default())
        .expect("compile");
    let v1 = compiler
        .compile_with_context(
            &interests,
            &CompileContext {
                indexer_set_version: 0,
                user_config_version: 1,
            },
        )
        .expect("compile");

    assert_ne!(
        v0.plan_id, v1.plan_id,
        "a bumped user_config_version must change the plan-id"
    );
    // `compile()` is exactly `compile_with_context(.., &default())`.
    let via_default = compiler.compile(&interests).expect("compile");
    assert_eq!(
        v0.plan_id, via_default.plan_id,
        "compile() must equal compile_with_context with a default context"
    );
}

// ── Gap 7: unroutable_authors is excluded from plan_id ──────────────────

/// Toggling `app_relays` flips an author between routable and unroutable,
/// but `app_relays` is deliberately NOT fed into `compute_plan_id`. So a
/// compile WITH app-relays and one WITHOUT — same interests, same mailbox
/// cache, same context — must share a plan-id even though their
/// `unroutable_authors` sets differ. (The wire-emitter diff must not
/// churn sub-ids when the operator toggles app relays at runtime.)
#[test]
fn app_relay_toggle_changes_unroutable_set_but_not_plan_id() {
    // Bob has no NIP-65 mailbox — his routability depends entirely on
    // whether app_relays are configured.
    let cache = InMemoryMailboxCache::new();
    let interests = [author_interest(
        1,
        &["bob"],
        &[1],
        InterestLifecycle::Tailing,
    )];

    // Without app relays: Bob is unroutable.
    let no_app = SubscriptionCompiler::new(&cache, &[]);
    let plan_no_app = no_app.compile(&interests).expect("compile");
    assert!(
        plan_no_app.unroutable_authors.contains(&pk("bob")),
        "with no app relays Bob must be unroutable"
    );

    // With app relays: Bob is routable.
    let app = vec!["wss://app".to_string()];
    let with_app = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);
    let plan_with_app = with_app.compile(&interests).expect("compile");
    assert!(
        plan_with_app.unroutable_authors.is_empty(),
        "with app relays configured Bob must be routable"
    );

    // The two plans differ in their unroutable set...
    assert_ne!(
        plan_no_app.unroutable_authors, plan_with_app.unroutable_authors,
        "the unroutable set genuinely differs between the two compiles"
    );
    // ...but the plan-id is identical — app_relays are excluded from the hash.
    assert_eq!(
        plan_no_app.plan_id, plan_with_app.plan_id,
        "toggling app_relays must not perturb the plan-id (it is excluded \
         from compute_plan_id — see Stage 4 comment in compile_with_context)"
    );
}

/// Counterpart to the app-relay-toggle test: a NIP-65 mailbox ARRIVAL for
/// the same author DOES change the plan-id. The mailbox snapshot for
/// referenced pubkeys feeds `compute_plan_id`, so moving an author out of
/// the unroutable set via NIP-65 (rather than via app-relays) correctly
/// invalidates the plan.
#[test]
fn nip65_arrival_changes_plan_id_even_via_unroutable_author() {
    let interests = [author_interest(
        1,
        &["bob"],
        &[1],
        InterestLifecycle::Tailing,
    )];

    // Before NIP-65: empty cache, Bob unroutable.
    let empty_cache = InMemoryMailboxCache::new();
    let before = SubscriptionCompiler::new(&empty_cache, &[])
        .compile(&interests)
        .expect("compile");
    assert!(before.unroutable_authors.contains(&pk("bob")));

    // After NIP-65: Bob's kind:10002 arrives in the cache.
    let mut cache_with_bob = InMemoryMailboxCache::new();
    cache_with_bob.put(pk("bob"), write_snapshot(&["wss://bob-write"]));
    let after = SubscriptionCompiler::new(&cache_with_bob, &[])
        .compile(&interests)
        .expect("compile");
    assert!(after.unroutable_authors.is_empty());

    assert_ne!(
        before.plan_id, after.plan_id,
        "a NIP-65 mailbox arrival for a referenced author must change the plan-id"
    );
}
