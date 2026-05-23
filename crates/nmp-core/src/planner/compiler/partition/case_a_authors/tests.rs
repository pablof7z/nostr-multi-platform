use crate::planner::{
    compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
    interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
    plan::{RoutingSource, UserConfiguredCategory},
};

fn pk(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn timeline_interest(id: u64, authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pk(a)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

/// NIP-65 known author + app_relays configured → REQ to UNION of both
/// sets; the shared URL records BOTH lanes.
#[test]
fn case_a_nip65_known_unions_with_app_relays() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-write".to_string(), "wss://shared".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let indexer: Vec<String> = vec![];
    let app = vec!["wss://app".to_string(), "wss://shared".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

    let plan = compiler.compile(&[timeline_interest(1, &["alice"])]).expect("compile");

    // NIP-65 lane only on the author-only URL.
    let alice_only = plan.per_relay.get("wss://alice-write").expect("alice-write");
    assert!(alice_only.role_tags.contains(&RoutingSource::Nip65));
    assert!(!alice_only
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

    // AppRelay lane only on the app-only URL.
    let app_only = plan.per_relay.get("wss://app").expect("app");
    assert!(app_only
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
    assert!(!app_only.role_tags.contains(&RoutingSource::Nip65));

    // Both lanes on the shared URL.
    let shared = plan.per_relay.get("wss://shared").expect("shared");
    assert!(shared.role_tags.contains(&RoutingSource::Nip65));
    assert!(shared
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

    // No author is unroutable.
    assert!(plan.unroutable_authors.is_empty());
}

/// NIP-65 unknown author + app_relays configured → REQ to app_relays
/// ONLY (no indexer fallback), AppRelay lane.
#[test]
fn case_a_nip65_unknown_routes_to_app_relays_only() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let app = vec!["wss://app".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

    let plan = compiler.compile(&[timeline_interest(1, &["bob"])]).expect("compile");

    // Indexer URL is NEVER consulted for content routing now.
    assert!(plan.per_relay.get("wss://purplepag.es").is_none());

    // App relay carries Bob with the AppRelay lane only.
    let app_plan = plan.per_relay.get("wss://app").expect("app");
    assert!(app_plan
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
    assert!(!app_plan
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));

    // Bob is NOT unroutable — app_relays carried him.
    assert!(plan.unroutable_authors.is_empty());
}

/// NIP-65 unknown author + no app_relays → author lands in
/// `unroutable_authors`; the indexer is NOT a fallback.
#[test]
fn case_a_no_nip65_no_app_relays_marks_author_unroutable() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let app: Vec<String> = vec![];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

    let plan = compiler.compile(&[timeline_interest(1, &["bob"])]).expect("compile");

    assert!(plan.per_relay.is_empty(), "no relays should be selected for content");
    assert!(
        plan.unroutable_authors.contains(&pk("bob")),
        "bob should be marked unroutable; got {:?}",
        plan.unroutable_authors
    );
}

/// Multi-author: one with NIP-65, one without; with app_relays both land
/// SOMEWHERE — neither is unroutable.
#[test]
fn case_a_mixed_nip65_known_and_unknown_with_app_relays() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-write".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let app = vec!["wss://app".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

    let plan = compiler.compile(&[timeline_interest(1, &["alice", "bob"])]).expect("compile");

    // Alice's write relay carries Alice via NIP-65 (and also AppRelay if Alice is there).
    let alice_plan = plan.per_relay.get("wss://alice-write").expect("alice-write");
    assert!(alice_plan.role_tags.contains(&RoutingSource::Nip65));

    // App relay carries both Alice and Bob via AppRelay lane.
    let app_plan = plan.per_relay.get("wss://app").expect("app");
    assert!(app_plan
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

    // No one is unroutable.
    assert!(plan.unroutable_authors.is_empty());
}

/// Multi-author: one with NIP-65, one without; no app_relays. Only the
/// known-mailbox author flies; the other lands in `unroutable_authors`.
#[test]
fn case_a_mixed_no_app_relays_isolates_unroutable() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-write".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let plan = compiler.compile(&[timeline_interest(1, &["alice", "bob"])]).expect("compile");

    // Alice flies.
    assert!(plan.per_relay.contains_key("wss://alice-write"));

    // Bob is unroutable.
    assert!(plan.unroutable_authors.contains(&pk("bob")));
    assert!(!plan.unroutable_authors.contains(&pk("alice")));
}

/// NIP-65 known but `outbox_relays()` is empty AND no app_relays → the
/// author is unroutable. (Empty mailbox is equivalent to missing one for
/// the purpose of routing content; the kernel surfaces it identically.)
#[test]
fn case_a_empty_mailbox_without_app_relays_is_unroutable() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let plan = compiler.compile(&[timeline_interest(1, &["alice"])]).expect("compile");

    assert!(plan.per_relay.is_empty());
    assert!(plan.unroutable_authors.contains(&pk("alice")));
}

// ── PD-033-C planner extension — indexer fallback arm (§4.3) ────────────
//
// The matrix below mirrors `kernel/discovery.rs::drain_unknown_oneshots`'s
// profile-oneshot arm: kind:0/3/10002 + authors → `RelayRole::Indexer`.
// Without this, deleting M1 in Stage 1 would mark every discovery-targeted
// pubkey `unroutable` and the kernel would never fetch the profile.

/// One-shot global profile fetch (the discovery-oneshot shape) with NO
/// NIP-65 mailbox cached AND NO app_relays → routes to
/// `bootstrap_indexer_relays` (lane `UserConfigured(Indexer)`). The author
/// is NOT `unroutable`. This is the headline silent-loss regression the
/// planner extension fixes.
#[test]
fn pd033c_case_a_oneshot_global_no_nip65_routes_to_bootstrap_indexer() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        /* indexer = */ &[],
        &[],
        &[],
        /* bootstrap_content = */ &[],
        &bootstrap_indexer,
    );

    // Profile-shape oneshot, scope Global — matches `oneshot.request(...)`.
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("bob")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    let ix = plan
        .per_relay
        .get("wss://purplepag.es")
        .expect("bootstrap indexer must carry the discovery profile-oneshot");
    assert!(ix
        .role_tags
        .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));
    // Critical: Bob is NOT unroutable — the silent-loss invariant.
    assert!(
        plan.unroutable_authors.is_empty(),
        "PD-033-C invariant: discovery-oneshot authors with bootstrap-indexer \
         fallback must NOT be marked unroutable; got {:?}",
        plan.unroutable_authors
    );
}

/// Cold-start divergence regression: `lifecycle.indexer_relays` (the raw
/// editable indexer rows) and `bootstrap_indexer_relays` (the kernel's
/// `bootstrap_urls_for_role(RelayRole::Indexer)`, which carries
/// `FALLBACK_INDEXER_RELAY` when no row is configured) are NOT
/// interchangeable. M1's profile-oneshot arm rides the WITH-fallback form;
/// the planner extension must do the same or cold-start sign-ins (no
/// indexer row configured yet) silently lose discovery the moment Stage 1
/// deletes M1. This test pins the divergence: raw indexer empty +
/// bootstrap_indexer non-empty → discovery still lands.
#[test]
fn pd033c_case_a_cold_start_uses_bootstrap_indexer_not_raw_indexer() {
    let cache = InMemoryMailboxCache::new();
    // The cold-start case: NO operator-configured indexer rows. Raw
    // `indexer_relays` is empty (the kernel's `set_relay_edit_rows` filter
    // returned nothing); `bootstrap_indexer_relays` carries the fallback.
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        /* indexer (raw, no fallback) = */ &[],
        &[],
        &[],
        /* bootstrap_content = */ &[],
        &bootstrap_indexer,
    );

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("bob")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    assert!(
        plan.per_relay.get("wss://purplepag.es").is_some(),
        "cold-start discovery MUST land on bootstrap_indexer even when raw \
         indexer_relays is empty (M1 parity)"
    );
    assert!(
        plan.unroutable_authors.is_empty(),
        "cold-start discovery author MUST NOT be unroutable"
    );
}

/// Counterpoint: a `Tailing` follow-feed interest (a non-discovery
/// timeline) for the same NIP-65-unknown author MUST still be `unroutable`
/// even when `bootstrap_indexer_relays` is set — the planner extension is
/// strictly scoped to discovery oneshots; broader fallback would degrade
/// routing for the 99% case (tailing follows ride NIP-65, indexer is
/// discovery-only per T134).
#[test]
fn pd033c_case_a_tailing_no_nip65_remains_unroutable() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &[],
        &bootstrap_indexer,
    );

    // Plain timeline interest — Tailing lifecycle, exactly the shape that
    // must NOT be diverted to the indexer (would re-introduce the T134
    // anti-pattern of follow-feeds on purplepag.es).
    let plan = compiler
        .compile(&[timeline_interest(1, &["bob"])])
        .expect("compile");

    assert!(
        plan.per_relay.get("wss://purplepag.es").is_none(),
        "Tailing follow-feed must NOT route to bootstrap indexer (T134 invariant)"
    );
    assert!(
        plan.unroutable_authors.contains(&pk("bob")),
        "Tailing+Global without NIP-65/app-relays must remain unroutable"
    );
}

/// Counterpoint: a `OneShot + Account(x)` profile fetch is account-scoped
/// (it ultimately resolves to a concrete account context). Today it stays
/// `unroutable` rather than diverting to the indexer — gate is OneShot AND
/// Global, not OneShot alone. This prevents account-scoped interests from
/// being mistakenly placed on the cold-start indexer lane.
#[test]
fn pd033c_case_a_account_scoped_oneshot_does_not_indexer_fallback() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &[],
        &bootstrap_indexer,
    );

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Account(pk("alice")),
        shape: InterestShape {
            authors: [pk("bob")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    assert!(
        plan.per_relay.get("wss://purplepag.es").is_none(),
        "Account-scoped OneShot must NOT divert to the bootstrap indexer lane"
    );
    assert!(plan.unroutable_authors.contains(&pk("bob")));
}

/// When `app_relays` ARE configured, the `if !landed` block never fires —
/// the AppRelay lane already carried the author. The PD-033-C
/// bootstrap-indexer arm must NOT additively route to the indexer in that
/// case (would double-charge the indexer for a routable author).
#[test]
fn pd033c_case_a_oneshot_global_with_app_relays_skips_bootstrap_indexer() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let app = vec!["wss://user-app.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &app,
        &[],
        &bootstrap_indexer,
    );

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("bob")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    // App relay carried Bob — indexer must be untouched.
    assert!(plan.per_relay.get("wss://user-app.example").is_some());
    assert!(
        plan.per_relay.get("wss://purplepag.es").is_none(),
        "PD-033-C bootstrap-indexer fallback must NOT fire when AppRelay \
         carried the author"
    );
    assert!(plan.unroutable_authors.is_empty());
}

/// Mixed multi-author: one author with NIP-65, one author without (and no
/// app_relays). The NIP-65 author rides their write relay; the
/// no-mailbox author falls back to the bootstrap indexer via the PD-033-C
/// arm. Critically: neither lands in `unroutable_authors`.
#[test]
fn pd033c_case_a_mixed_authors_partial_nip65_landed_via_bootstrap_indexer() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), MailboxSnapshot {
        write_relays: vec!["wss://alice-write".to_string()],
        read_relays: vec![],
        both_relays: vec![],
    });
    let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &[],
        &bootstrap_indexer,
    );

    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("alice"), pk("bob")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    // Alice rides her NIP-65 write relay.
    assert!(plan.per_relay.get("wss://alice-write").is_some());
    // Bob lands on the bootstrap indexer via the PD-033-C arm.
    assert!(plan.per_relay.get("wss://purplepag.es").is_some());
    // Neither is unroutable.
    assert!(plan.unroutable_authors.is_empty());
}
