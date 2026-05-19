//! M2 subscription compiler audit gate — phase 1 subset.
//!
//! Exercises the `nmp_core::planner` module: `SubscriptionCompiler`,
//! `LogicalInterest`, `NaddrCoord`, and the merge lattice.
//!
//! Corresponds to the contracts in
//! `docs/design/subscription-compilation/tests.md` §9.2, Assertions 2 & 5.
//! Assertions 1, 3, 4 depend on infrastructure (AppActionMeta, trigger replay,
//! four-lane diagnostics) that lands in later slices.
//!
//! CI gate: `cargo test -p nmp-testing --test m2_subscription_compilation_audit`
//!
//! Design: `docs/design/subscription-compilation/tests.md`
//! Doctrine: D3 (routing automatic), D6 (errors internal), D8 (zero allocs).

// Import through the planner's public API surface — submodule paths are
// pub(crate) and must not be named from an external crate.
use nmp_core::planner::{
    InMemoryMailboxCache,
    MailboxSnapshot,
    SubscriptionCompiler,
    InterestId,
    InterestLifecycle,
    InterestScope,
    InterestShape,
    LogicalInterest,
    NaddrCoord,
};
use std::collections::BTreeSet;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    // Deterministic 64-char hex pubkey from a short seed string.
    format!("{seed:0>64}")
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase()
}

fn relay(url: &str) -> String {
    url.to_string()
}

fn make_authors_with_overlapping_mailboxes(
    count: usize,
) -> Vec<(String, MailboxSnapshot)> {
    // Three relay groups with deliberate overlap:
    //   - first 60% of authors → relay.damus.io + nos.lol
    //   - next  30% of authors → nostr.wine     + nos.lol
    //   - last  10% of authors → operator-niche.example
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let pk = pubkey(&format!("author{i:06}"));
        let mb = if i < (count * 6 / 10) {
            MailboxSnapshot {
                write_relays: vec![relay("wss://relay.damus.io"), relay("wss://nos.lol")],
                read_relays: vec![],
                both_relays: vec![],
            }
        } else if i < (count * 9 / 10) {
            MailboxSnapshot {
                write_relays: vec![relay("wss://nostr.wine"), relay("wss://nos.lol")],
                read_relays: vec![],
                both_relays: vec![],
            }
        } else {
            MailboxSnapshot {
                write_relays: vec![relay("wss://operator-niche.example")],
                read_relays: vec![],
                both_relays: vec![],
            }
        };
        result.push((pk, mb));
    }
    result
}

fn interest_id(n: u64) -> InterestId {
    InterestId(n)
}

// ─── Assertion 2 — per-author wire fan-out ────────────────────────────────────

/// "For a timeline of N authors, the compiled plan opens REQs only against
/// the union of those authors' write relays (de-duplicated). Each relay carries
/// exactly one merged sub-shape."
///
/// Design: `docs/design/subscription-compilation/tests.md` §9.2 Assertion 2.
#[test]
fn timeline_compiles_to_per_relay_union() {
    // Design spec §9.2 Assertion 2 states 1000 authors as the boundary.
    let authors = make_authors_with_overlapping_mailboxes(1000);

    let mut cache = InMemoryMailboxCache::new();
    for (pk, mb) in &authors {
        cache.put(pk.clone(), mb.clone());
    }

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let author_set: BTreeSet<String> = authors.iter().map(|(pk, _)| pk.clone()).collect();
    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            authors: author_set.clone(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    // Assert: relay set == union of declared write relays (no extras, no misses).
    let expected_relays: BTreeSet<String> = authors
        .iter()
        .flat_map(|(_, mb)| mb.write_relays.iter().cloned())
        .collect();
    let actual_relays: BTreeSet<String> = plan.per_relay.keys().cloned().collect();
    assert_eq!(
        actual_relays, expected_relays,
        "compiled relay set must equal the union of declared write relays"
    );

    // Assert: each relay carries exactly one sub-shape (merge succeeded).
    for (url, rp) in &plan.per_relay {
        assert_eq!(
            rp.sub_shapes.len(),
            1,
            "relay {url} should have one merged sub-shape, got {}",
            rp.sub_shapes.len()
        );
    }

    // Assert: each relay's authors are exactly the subset that declared it.
    for (url, rp) in &plan.per_relay {
        let expected_authors: BTreeSet<String> = authors
            .iter()
            .filter(|(_, mb)| mb.write_relays.contains(url))
            .map(|(pk, _)| pk.clone())
            .collect();
        let actual_authors = &rp.sub_shapes[0].shape.authors;
        assert_eq!(
            actual_authors, &expected_authors,
            "relay {url} should serve only its declared authors"
        );
    }

    // Assert: plan-id is stable — two consecutive compiles with no input change.
    let plan2 = compiler.compile(&[LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            authors: author_set,
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    }]).expect("compile #2");
    assert_eq!(
        plan.plan_id, plan2.plan_id,
        "re-compile with identical inputs must yield the same plan_id"
    );
}

// ─── Assertion 5 — address-pointer dedup ─────────────────────────────────────

/// "Two views registering the same NaddrCoord emit ONE REQ per relay (Rule 8
/// address-pointer union, D8 invariant)."
///
/// Design: `docs/design/subscription-compilation/tests.md` §9.2 Assertion 5.
#[test]
fn address_pointer_dedup_across_two_interests() {
    let article_pk = pubkey("article_author");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        article_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://article-relay.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let coord = NaddrCoord {
        pubkey: article_pk,
        kind: 30023,
        d_tag: "my-post".to_string(),
    };

    // Two interests (ThreadView + Nip10ModularTimelineView in nmp-nip01) for the same coord.
    let make_interest = |id: u64| LogicalInterest {
        id: interest_id(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            addresses: [coord.clone()].into_iter().collect(),
            kinds: [30023u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler
        .compile(&[make_interest(10), make_interest(11)])
        .expect("compile");

    // Assert: one relay in the plan (the article author's write relay).
    assert_eq!(
        plan.per_relay.len(),
        1,
        "address-pointer interests for the same author route to exactly one relay"
    );

    // Assert: Rule 8 merged them into one SubShape.
    let relay_plan = plan.per_relay.values().next().unwrap();
    assert_eq!(
        relay_plan.sub_shapes.len(),
        1,
        "Rule 8 must merge identical address sets into one SubShape"
    );

    // Assert: merged sub-shape contains the union of both interests' address sets.
    // (Both interests had the same coord, so the union is that one coord.)
    let sub = &relay_plan.sub_shapes[0];
    assert!(
        sub.shape.addresses.contains(&coord),
        "merged SubShape must contain the NaddrCoord from both interests"
    );
    assert_eq!(
        sub.shape.addresses.len(),
        1,
        "dedup: merged address set must have exactly one coord (union of two identical sets)"
    );

    // Assert: both originating interests are tracked in the merged plan output.
    // D8 invariant: the reverse index must account for all claim holders.
    let mut tracked_ids: Vec<u64> = sub.originating_interests.iter().map(|id| id.0).collect();
    tracked_ids.sort();
    assert_eq!(
        tracked_ids,
        vec![10, 11],
        "both originating InterestIds must be recorded in the merged SubShape"
    );
}

// ─── Unroutable when no mailbox AND no app_relays ────────────────────────────

/// Authors with no known mailbox AND no `app_relays` configured route to
/// `CompiledPlan::unroutable_authors` — the indexer is for discovery only,
/// NEVER a content fallback (T134; see `feat(planner): app_relays + drop
/// indexer fallback for content`).
#[test]
fn unknown_author_with_no_app_relays_becomes_unroutable() {
    let cache = InMemoryMailboxCache::new(); // empty
    let indexer = vec![relay("wss://purplepag.es")];
    // SubscriptionCompiler::new defaults app_relays = &[] — so an author with no
    // mailbox AND no app_relays has no content lane and must land in
    // `unroutable_authors`, not on the indexer.
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let interest = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("unknown_alice")].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    // Assert: unknown author is recorded in `unroutable_authors` so the
    // kernel can surface a "no relays to fetch from" toast.
    assert!(
        plan.unroutable_authors.contains(&pubkey("unknown_alice")),
        "unknown author with no app_relays must be recorded in unroutable_authors"
    );
    // Assert: indexer is NOT used as a content fallback.
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "indexer must NOT receive content REQs for unknown authors (T134: discovery-only)"
    );
}

// ─── Authors + addresses combined in one interest ────────────────────────────

/// When an interest declares BOTH explicit authors AND address-pointer coordinates,
/// the compiled plan must include both in the per-relay sub-shape.
///
/// Regression: an early Case-A implementation dropped addresses when authors
/// were non-empty (it returned early before processing `interest.shape.addresses`).
#[test]
fn interest_with_authors_and_addresses_preserves_both() {
    let author_pk = pubkey("author_with_relay");
    let article_pk = pubkey("article_author");

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        author_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://author-relay.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache.put(
        article_pk.clone(),
        MailboxSnapshot {
            write_relays: vec![relay("wss://author-relay.example")],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    let indexer = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let coord = NaddrCoord {
        pubkey: article_pk.clone(),
        kind: 30023,
        d_tag: "my-article".to_string(),
    };

    // A single interest with both authors and addresses.
    let interest = LogicalInterest {
        id: interest_id(99),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [author_pk.clone()].into_iter().collect(),
            addresses: [coord.clone()].into_iter().collect(),
            kinds: [1u32, 30023u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler.compile(&[interest]).expect("compile");

    // Assert: the relay is present in the plan.
    assert!(
        plan.per_relay.contains_key("wss://author-relay.example"),
        "author's declared relay must appear in the plan"
    );

    // Assert: the sub-shape retains the address coordinate (not dropped by Case A).
    let rp = &plan.per_relay["wss://author-relay.example"];
    assert_eq!(rp.sub_shapes.len(), 1, "should have one merged sub-shape");
    assert!(
        rp.sub_shapes[0].shape.addresses.contains(&coord),
        "address coordinate must be preserved in the sub-shape when authors are also present"
    );
}

// ─── Plan-id stability under repeated compile ─────────────────────────────────

/// Compiling the same interests twice without any state change must produce
/// the same plan_id (D8 idempotency).
#[test]
fn plan_id_stable_under_repeated_compile() {
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

    let interests = vec![LogicalInterest {
        id: interest_id(42),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    }];

    let plan_a = compiler.compile(&interests).expect("compile a");
    let plan_b = compiler.compile(&interests).expect("compile b");

    assert_eq!(
        plan_a.plan_id, plan_b.plan_id,
        "identical inputs must produce identical plan_id"
    );
}

// ─── Plan-id changes when interests change ────────────────────────────────────

/// Adding a new interest must change the plan_id even if no new wire REQ results
/// from the merge.
#[test]
fn plan_id_changes_when_interest_set_changes() {
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

    let interest_a = LogicalInterest {
        id: interest_id(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };
    let interest_b = LogicalInterest {
        id: interest_id(2),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan_one = compiler.compile(std::slice::from_ref(&interest_a)).expect("compile one");
    let plan_two = compiler
        .compile(&[interest_a, interest_b])
        .expect("compile two");

    assert_ne!(
        plan_one.plan_id, plan_two.plan_id,
        "adding an interest must change plan_id even if wire REQs merge"
    );
}

// Plan-id referenced-pubkeys-only tests live in m2_plan_id_stability.rs
// to keep this file under the 500-LOC hard limit.
