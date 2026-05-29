use super::*;
use crate::compiler::mailbox::{InMemoryMailboxCache, MailboxSnapshot};
use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
};
use std::collections::BTreeSet;

/// Deterministic 64-char hex pubkey fixture from a short label.
fn pk(label: &str) -> String {
    format!("{label:0>64}").chars().take(64).collect()
}

/// A NIP-65 snapshot whose write relays are the given URLs.
fn write_snapshot(write: &[&str]) -> MailboxSnapshot {
    MailboxSnapshot {
        write_relays: write.iter().map(|s| s.to_string()).collect(),
        read_relays: vec![],
        both_relays: vec![],
    }
}

/// A tailing author+kind interest. `kinds` lets callers force a merge
/// refusal (Rule 1) by giving two interests different kind sets.
fn author_interest(
    id: u64,
    authors: &[&str],
    kinds: &[u32],
    lifecycle: InterestLifecycle,
) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pk(a)).collect(),
            kinds: kinds.iter().copied().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle,
        is_indexer_discovery: false,
    }
}

// ── Gap 1: empty interests → empty plan ─────────────────────────────────

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

// ── Gap 2: single author interest → correct filter shape ────────────────

/// One author with a known NIP-65 write relay produces exactly one
/// `RelayPlan` carrying exactly one `SubShape`, whose shape echoes the
/// interest's authors+kinds and names the originating interest.
#[test]
fn single_author_interest_produces_one_subshape() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[author_interest(
            1,
            &["alice"],
            &[1],
            InterestLifecycle::Tailing,
        )])
        .expect("compile");

    assert_eq!(plan.per_relay.len(), 1, "exactly one relay in the plan");
    let relay = plan
        .per_relay
        .get("wss://alice-write")
        .expect("alice-write relay");
    assert_eq!(relay.sub_shapes.len(), 1, "one interest → one sub-shape");

    let sub = &relay.sub_shapes[0];
    // Author-partitioning: the sub-shape's author set is exactly Alice.
    assert_eq!(
        sub.shape.authors,
        [pk("alice")].into_iter().collect::<BTreeSet<_>>()
    );
    assert_eq!(sub.shape.kinds, [1u32].into_iter().collect::<BTreeSet<_>>());
    // Provenance: the sub-shape names interest #1.
    assert_eq!(sub.originating_interests, vec![InterestId(1)]);
    // The cached hash matches a fresh hash of the shape.
    assert_eq!(sub.canonical_filter_hash, canonical_filter_hash(&sub.shape));
}

// ── Gap 3: two compatible interests for the same relay → merged ─────────

/// Two interests with mergeable shapes (same kinds, same lifecycle) that
/// route to the SAME relay collapse into a single `SubShape`. Stage 3's
/// greedy merge unions the author sets and records BOTH originating
/// interest ids on the one sub-shape.
#[test]
fn two_compatible_interests_same_relay_merge_into_one_subshape() {
    let mut cache = InMemoryMailboxCache::new();
    // Two distinct authors, both publishing to the same write relay.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        1,
        "two mergeable interests on one relay collapse into one REQ"
    );
    let sub = &relay.sub_shapes[0];
    // Merged shape unions both authors.
    assert_eq!(
        sub.shape.authors,
        [pk("alice"), pk("bob")]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
    // Both interest ids are recorded on the merged sub-shape.
    let ids: BTreeSet<InterestId> = sub.originating_interests.iter().cloned().collect();
    assert_eq!(ids, [InterestId(1), InterestId(2)].into_iter().collect());
}

// ── Gap 3 (refusal): two incompatible interests → two sub-shapes ────────

/// Two interests that route to the same relay but FAIL the merge lattice
/// (here Rule 1 — different kind sets) produce TWO distinct `SubShape`s
/// on the one `RelayPlan`: one wire REQ each.
#[test]
fn incompatible_kinds_same_relay_stay_distinct_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            // kind:1 — text notes.
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            // kind:30023 — long-form. Rule 1 refuses (distinct, no wildcard).
            author_interest(2, &["bob"], &[30023], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "incompatible kind sets must NOT merge — two REQs on the relay"
    );
}

/// Two interests on the same relay with different LIFECYCLES (Tailing vs
/// OneShot) fail Rule 6 and stay as two `SubShape`s — the wire-emitter
/// needs distinct frames so it can CLOSE the one-shot REQ on EOSE while
/// leaving the tailing one open.
#[test]
fn mixed_lifecycle_same_relay_stays_distinct_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::OneShot),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "Rule 6 refuses cross-lifecycle merges — two REQs on the relay"
    );
}

// ── Gap 4: originating_interests dedup ──────────────────────────────────

/// An interest with explicit `authors` AND `#p` tag values fires both the
/// Case A outbox push and the "both populated" inbox push. When the
/// author's write relay and the tagged pubkey's read relay are the SAME
/// URL, the one interest_id lands on that relay twice — Stage 3 must
/// record it only once (`originating_interests` is a set, not a multiset).
#[test]
fn same_interest_on_one_relay_via_two_lanes_dedupes_originating_id() {
    let mut cache = InMemoryMailboxCache::new();
    // Alice (the author) writes to wss://shared.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    // Carol (the #p-tagged recipient) READS from the very same wss://shared.
    cache.put(
        pk("carol"),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec!["wss://shared".to_string()],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    // One interest: author Alice + #p:[Carol].
    let mut tags = std::collections::BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [pk("carol")].into_iter().collect::<BTreeSet<_>>(),
    );
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("alice")].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    let relay = plan.per_relay.get("wss://shared").expect("shared relay");

    // Across ALL sub-shapes on the relay, interest #1 must appear exactly
    // once per sub-shape's originating list — never duplicated.
    for sub in &relay.sub_shapes {
        let count = sub
            .originating_interests
            .iter()
            .filter(|id| **id == InterestId(1))
            .count();
        assert!(
            count <= 1,
            "interest id must be deduped within a sub-shape; saw it {count} times"
        );
    }
}

// ── Gap 5: role_tags accumulation across distinct interests ─────────────

/// One relay reached by two different interests via two different lanes
/// (author A via NIP-65, author B via AppRelay because the operator
/// pinned the same URL) must carry BOTH lanes in `role_tags` — the
/// four-lane discipline is preserved across interest boundaries, not just
/// within one interest.
#[test]
fn role_tags_accumulate_across_interests_on_a_shared_relay() {
    let mut cache = InMemoryMailboxCache::new();
    // Alice declares wss://shared as her NIP-65 write relay.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    // Bob has no mailbox; he will only ride the app-relay lane.
    let app = vec!["wss://shared".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert!(
        relay.role_tags.contains(&RoutingSource::Nip65),
        "Alice's NIP-65 lane must be recorded"
    );
    assert!(
        relay.role_tags.contains(&RoutingSource::UserConfigured(
            crate::plan::UserConfiguredCategory::AppRelay
        )),
        "Bob's AppRelay lane must be recorded on the same relay"
    );
}

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

// ── Mixed-shape interests on one relay (timeline + profile) ─────────────

/// A timeline interest (kinds {1,6}, no limit) and a profile interest
/// (kinds {0,3,10002}, limit Some(3)) for the SAME author route to the
/// same write relay but cannot merge — different kinds (Rule 1) and a
/// limit on one side (Rule 5). The relay therefore carries two distinct
/// sub-shapes, each with the correct filter shape.
#[test]
fn timeline_and_profile_for_same_author_produce_two_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let timeline = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape::timeline_for(
            [pk("alice")].into_iter().collect(),
            [30023u32].into_iter().collect(),
        ),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    let profile = LogicalInterest {
        id: InterestId(2),
        scope: InterestScope::Global,
        shape: InterestShape::profile_for(pk("alice")),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[timeline, profile]).expect("compile");
    let relay = plan
        .per_relay
        .get("wss://alice-write")
        .expect("alice-write relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "timeline and profile shapes cannot merge — two REQs on the relay"
    );

    // Exactly one sub-shape carries the timeline kinds, one the profile kinds.
    // V-68: `timeline_for` no longer injects {1, 6}; this test declares an
    // arbitrary host kind set ({30023}) to prove the constructor carries
    // caller policy verbatim and the compiler routes it without rewriting.
    let timeline_kinds: BTreeSet<u32> = [30023].into_iter().collect();
    let profile_kinds: BTreeSet<u32> = [0, 3, 10002].into_iter().collect();
    let has_timeline = relay
        .sub_shapes
        .iter()
        .any(|s| s.shape.kinds == timeline_kinds);
    let has_profile = relay
        .sub_shapes
        .iter()
        .any(|s| s.shape.kinds == profile_kinds);
    assert!(
        has_timeline,
        "one sub-shape must carry the host-declared timeline kinds {{30023}}"
    );
    assert!(
        has_profile,
        "one sub-shape must carry the profile kinds {{0,3,10002}}"
    );

    // The profile sub-shape preserves its limit (Rule 5 would have refused
    // any merge that dropped it).
    let profile_sub = relay
        .sub_shapes
        .iter()
        .find(|s| s.shape.kinds == profile_kinds)
        .expect("profile sub-shape");
    assert_eq!(
        profile_sub.shape.limit,
        Some(3),
        "profile limit must survive"
    );
}

/// A naddr-coordinate address pointer (Case B) routes to the addressed
/// author's write relay and produces a sub-shape whose `addresses` field
/// carries the coordinate verbatim.
#[test]
fn address_pointer_interest_routes_coord_to_authors_write_relay() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("author"), write_snapshot(&["wss://author-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let coord = NaddrCoord {
        pubkey: pk("author"),
        kind: 30023,
        d_tag: "long-form".to_string(),
    };
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [30023u32].into_iter().collect(),
            addresses: [coord.clone()].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    let relay = plan
        .per_relay
        .get("wss://author-write")
        .expect("author-write relay");
    assert_eq!(relay.sub_shapes.len(), 1, "one address pointer → one REQ");
    assert!(
        relay.sub_shapes[0].shape.addresses.contains(&coord),
        "the sub-shape must carry the naddr coordinate verbatim"
    );
}
