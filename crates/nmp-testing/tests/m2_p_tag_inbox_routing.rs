//! M2 audit: `#p` tag inbox routing — Case A "Both populated" split + Case C
//! fail-closed structural ban.
//!
//! Exercises `route_p_tags_to_inbox` correctness on:
//! 1. interests with both `authors` AND `#p` (split outbox/inbox per spec §3.1)
//! 2. preserving the original author constraint on the inbox slice
//! 3. fail-closed when tagged pubkey's inbox relays are unknown
//! 4. dedupe of duplicate `interest_id` when outbox + inbox land on the same relay
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, SubscriptionCompiler,
};
use std::collections::{BTreeMap, BTreeSet};

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

// ─── Test 1 — authors+#p produces both Outbox AND Inbox entries ──────────────

/// An interest `{authors:[Alice], #p:[Bob]}` must route to BOTH Alice's
/// write relays (Outbox) AND Bob's read relays (Inbox), per spec §3.1
/// "Both populated" row.
///
/// Before T33-round-2: Case A returned before Case C, losing the inbox slice.
#[test]
fn authors_plus_p_tag_emits_both_outbox_and_inbox() {
    let alice = pubkey("alice");
    let bob = pubkey("bob");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        alice.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice-write.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache.put(
        bob.clone(),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![relay("wss://bob-read.example")],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert("p".to_string(), [bob.clone()].into_iter().collect());

    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [alice.clone()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    // Outbox: Alice's write relay must be present.
    assert!(
        plan.per_relay.contains_key("wss://alice-write.example"),
        "outbox: author's write relay must appear"
    );
    // Inbox: Bob's read relay must also be present (T33 fix).
    assert!(
        plan.per_relay.contains_key("wss://bob-read.example"),
        "inbox: tagged pubkey's read relay must appear (Both populated split)"
    );
}

// ─── Test 2 — inbox slice preserves the original author constraint ───────────

/// The inbox split must keep `authors=[Alice]` on the inbox shape; without
/// it, the REQ becomes "every event tagging Bob" instead of "Alice's events
/// tagging Bob" — wrong semantics (codex round-3 P1 finding).
#[test]
fn inbox_split_preserves_original_authors() {
    let alice = pubkey("alice");
    let bob = pubkey("bob");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        alice.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://alice-write.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache.put(
        bob.clone(),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![relay("wss://bob-read.example")],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert("p".to_string(), [bob.clone()].into_iter().collect());

    let interest = LogicalInterest {
        id: interest_id(2),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [alice.clone()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    let bob_inbox = &plan.per_relay["wss://bob-read.example"];
    assert_eq!(
        bob_inbox.sub_shapes.len(),
        1,
        "one inbox sub-shape on Bob's read"
    );
    let inbox_shape = &bob_inbox.sub_shapes[0].shape;
    assert!(
        inbox_shape.authors.contains(&alice),
        "inbox slice MUST carry `authors=[Alice]` so the REQ is filtered to Alice's events (not all #p:Bob events)"
    );
    assert_eq!(
        inbox_shape.tags.get("p").map(|s| s.len()),
        Some(1),
        "inbox slice must still carry #p:[Bob]"
    );
}

// ─── Test 3 — overlapping outbox/inbox relay dedupes interest_id ─────────────

/// When Alice's write relay is the SAME URL as Bob's read relay, Case A
/// pushes both an outbox entry AND an inbox entry on that relay for the
/// same interest. Stage 3 must dedupe so `originating_interests` has the
/// interest id exactly once (it's a set semantically) — codex round-3 P2.
#[test]
fn overlapping_outbox_inbox_relay_dedupes_interest_id() {
    let alice = pubkey("alice");
    let bob = pubkey("bob");
    let shared = relay("wss://shared.example");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        alice.clone(),
        MailboxSnapshot {
            write_relays: vec![shared.clone()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache.put(
        bob.clone(),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![shared.clone()],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert("p".to_string(), [bob.clone()].into_iter().collect());

    let interest = LogicalInterest {
        id: interest_id(3),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [alice.clone()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    let shared_plan = &plan.per_relay["wss://shared.example"];
    // After Stage 3 merge of compatible shapes, there should be one sub-shape
    // (outbox and inbox merged because lifecycle/kinds/tags are the same).
    assert_eq!(
        shared_plan.sub_shapes.len(),
        1,
        "outbox + inbox on the same relay must merge into one sub-shape"
    );
    // The originating_interests list must contain the id exactly once.
    let ids = &shared_plan.sub_shapes[0].originating_interests;
    let id_count = ids.iter().filter(|id| **id == InterestId(3)).count();
    assert_eq!(
        id_count, 1,
        "interest_id appears once in originating_interests (it's a set, not a multiset)"
    );
}

// ─── Test 4 — Case C fail-closed when inbox relays are unknown ───────────────

/// A `#p`-only interest where the tagged pubkey has no known mailbox must
/// produce ZERO relay entries (fail-closed). It must NOT fall back to the
/// indexer set, which would route the inbox query to a public relay without
/// the recipient's explicit declaration (structural ban, §3.1 / §3.2).
#[test]
fn p_tag_unknown_inbox_fails_closed_no_indexer_fallback() {
    let bob = pubkey("bob");

    // Cache has no entry for Bob.
    let cache = InMemoryMailboxCache::new();
    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert("p".to_string(), [bob.clone()].into_iter().collect());

    let interest = LogicalInterest {
        id: interest_id(4),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        plan.per_relay.is_empty(),
        "fail-closed: no relay entries when #p inbox is unknown (must NOT fall back to indexer)"
    );
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "indexer relay must NOT appear for unknown #p inbox (structural ban)"
    );
}

// ─── Test 5 — Case C uses both_relays as inbox when read_relays is empty ─────

/// `inbox_relays()` is `read_relays ∪ both_relays`. A pubkey with only
/// `both_relays` (no explicit reads) must still produce inbox entries.
#[test]
fn p_tag_inbox_uses_both_relays_when_no_explicit_reads() {
    let bob = pubkey("bob");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        bob.clone(),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![],
            both_relays: vec![relay("wss://bob-both.example")],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert("p".to_string(), [bob.clone()].into_iter().collect());

    let interest = LogicalInterest {
        id: interest_id(5),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [4u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        plan.per_relay.contains_key("wss://bob-both.example"),
        "both_relays must count as inbox targets (inbox = read ∪ both)"
    );
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "indexer must NOT be used when inbox relays (even just both_relays) exist"
    );
}

// ─── Test 6 — multi-#p with mixed known/unknown inbox scopes per-pubkey ─────

/// `#p=[Bob, Carol]` where Bob has inbox relays and Carol doesn't.
/// - Bob's relay must receive a REQ with `#p=[Bob]` only (NOT `[Bob, Carol]`)
///   so we don't over-fetch Carol-tagged events on Bob's relay.
/// - Carol must trigger `request_probe` and produce no relay entries
///   (fail-closed, per the structural ban).
#[test]
fn multi_p_tag_scopes_filter_per_tagged_pubkey() {
    let bob = pubkey("bob");
    let carol = pubkey("carol");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        bob.clone(),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![relay("wss://bob-read.example")],
            both_relays: vec![],
        },
    );
    // Carol has no mailbox in cache → fail-closed for Carol.

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [bob.clone(), carol.clone()].into_iter().collect(),
    );

    let interest = LogicalInterest {
        id: interest_id(6),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [4u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    // Bob's relay must appear with #p scoped to {Bob} only.
    let bob_inbox = plan
        .per_relay
        .get("wss://bob-read.example")
        .expect("Bob's read relay must appear");
    assert_eq!(bob_inbox.sub_shapes.len(), 1, "one sub-shape on Bob's read");
    let bob_p = bob_inbox.sub_shapes[0]
        .shape
        .tags
        .get("p")
        .expect("Bob's relay sub-shape must have a #p filter");
    assert_eq!(
        bob_p.len(),
        1,
        "Bob's relay #p must be scoped to exactly one pubkey (the relay's owner)"
    );
    assert!(
        bob_p.contains(&bob),
        "Bob's relay #p must contain Bob (not Carol — that would leak Carol's tag)"
    );
    assert!(
        !bob_p.contains(&carol),
        "Bob's relay must NOT see Carol in #p (per-pubkey scoping)"
    );

    // Total relay count: only Bob's relay (Carol's unknown inbox fails closed).
    assert_eq!(
        plan.per_relay.len(),
        1,
        "only Bob's relay should appear; Carol's unknown inbox fails closed"
    );
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "indexer must NOT be used as fallback for Carol's unknown inbox"
    );
}
