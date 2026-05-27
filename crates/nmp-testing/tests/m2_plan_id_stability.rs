//! M2 plan-id stability tests: §3.4 "referenced-pubkeys only" invariant.
//!
//! These tests verify that the plan-id hash covers ONLY the pubkeys that are
//! referenced by the interest set (authors, #p tags, address pubkeys), not the
//! entire mailbox cache. This was the core bug in the T26 implementation.
//!
//! Split from `m2_subscription_compilation_audit.rs` for the 500-LOC limit.
//!
//! CI gate: `cargo test -p nmp-testing --test m2_plan_id_stability`
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.4
//! Doctrine: D8 (plan-id stability avoids redundant recompilation).

use nmp_core::planner::{
    CompileContext, InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, MailboxSnapshot, SubscriptionCompiler,
};

// ─── Helpers (duplicated from audit to keep files independent) ────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}")
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase()
}

fn relay(url: &str) -> String {
    url.to_string()
}

fn interest_id(n: u64) -> InterestId {
    InterestId(n)
}

// ─── Plan-id stability: unrelated mailbox arrival ────────────────────────────

/// An unrelated kind:10002 arrival — for a pubkey NOT in any interest's
/// author set, #p tags, or address pubkeys — MUST NOT change the plan-id.
///
/// This tests the §3.4 "referenced-pubkeys only" invariant that was violated
/// by the original T26 implementation (which hashed the ENTIRE mailbox cache).
///
/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
#[test]
fn plan_id_unchanged_when_unrelated_mailbox_arrives() {
    let alice_pk = pubkey("alice");
    let unrelated_pk = pubkey("unrelated_stranger");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        alice_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let ctx = CompileContext::default();

    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [alice_pk.clone()].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    let plan_before = {
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        compiler
            .compile_with_context(std::slice::from_ref(&interest), &ctx)
            .expect("compile before")
    };

    // A kind:10002 arrives for unrelated_stranger — NOT in the interest's author set.
    cache.put(
        unrelated_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://stranger.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let plan_after = {
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        compiler
            .compile_with_context(std::slice::from_ref(&interest), &ctx)
            .expect("compile after")
    };

    assert_eq!(
        plan_before.plan_id, plan_after.plan_id,
        "unrelated mailbox arrival (pubkey not in interest set) must NOT change plan_id"
    );
}

// ─── Plan-id stability: referenced author mailbox update ─────────────────────

/// A kind:10002 update for a pubkey that IS in the interest's author set
/// MUST change the plan-id (the compiler must re-route).
///
/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
#[test]
fn plan_id_changes_when_referenced_author_mailbox_updates() {
    let alice_pk = pubkey("alice");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        alice_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice-old.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let ctx = CompileContext::default();

    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [alice_pk.clone()].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    let plan_before = {
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        compiler
            .compile_with_context(std::slice::from_ref(&interest), &ctx)
            .expect("compile before")
    };

    // Alice publishes a new kind:10002 pointing to a different relay.
    cache.put(
        alice_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice-new.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let plan_after = {
        let compiler = SubscriptionCompiler::new(&cache, &indexer);
        compiler
            .compile_with_context(std::slice::from_ref(&interest), &ctx)
            .expect("compile after")
    };

    assert_ne!(
        plan_before.plan_id, plan_after.plan_id,
        "mailbox update for a referenced author MUST change plan_id"
    );
}

// ─── Plan-id stability: indexer set version bump ────────────────────────────

/// Bumping `indexer_set_version` in the compile context must change plan-id
/// even when the interest set and mailbox cache are identical.
///
/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
#[test]
fn plan_id_changes_on_indexer_set_version_bump() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("alice"),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    let ctx_v0 = CompileContext {
        indexer_set_version: 0,
        user_config_version: 0,
    };
    let ctx_v1 = CompileContext {
        indexer_set_version: 1,
        user_config_version: 0,
    };

    let plan_v0 = compiler
        .compile_with_context(std::slice::from_ref(&interest), &ctx_v0)
        .expect("compile v0");
    let plan_v1 = compiler
        .compile_with_context(std::slice::from_ref(&interest), &ctx_v1)
        .expect("compile v1");

    assert_ne!(
        plan_v0.plan_id, plan_v1.plan_id,
        "indexer_set_version bump MUST change plan_id"
    );
}
