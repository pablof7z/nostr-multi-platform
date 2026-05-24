//! PD-033-C planner extension — Case C bootstrap-content inbox fallback.
//!
//! Mirrors the matrix in `case_d_no_author.rs::pd033c_*` (Stage 1
//! precedent): positive route, scope=Account counterpoint, lifecycle=OneShot
//! counterpoint, p_tag_routing=Nip17DmRelays counterpoint (fail-closed
//! preserved), partial inbox cache counterpoint (gate refuses), empty
//! bootstrap counterpoint (fall through to fail-closed), and plan_id
//! stability under bootstrap toggle.
//!
//! The headline contract: a `Tailing + Global + #p (Nip65ReadRelays)`
//! interest whose tagged pubkey has no cached NIP-65 inbox AND
//! `bootstrap_content_relays` is non-empty routes to the bootstrap content
//! lane, lane = `UserConfigured(Bootstrap)`. This is the silent-loss
//! regression Stage 2 of PD-033-C exposes for the kernel's self-zap-receipts
//! subscription (`kind:9735 #p=[self_pk]` on `RelayRole::Content`).
use crate::{
    compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
    interest::{
        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        PTagRouting,
    },
    plan::{RoutingSource, UserConfiguredCategory},
};
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic 64-char hex pubkey fixture from a short label.
fn pk(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// Build a `#p`-only interest with the given `p_tag_routing` mode.
/// Defaults to kind:9735 (the self-zap-receipts shape) and the canonical
/// `Tailing + Global` lifecycle/scope that the dispatcher gate keys on.
fn p_tag_interest(
    id: u64,
    tagged: &[&str],
    routing: PTagRouting,
    lifecycle: InterestLifecycle,
    scope: InterestScope,
) -> LogicalInterest {
    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let values: BTreeSet<String> = tagged.iter().map(|p| pk(p)).collect();
    tags.insert("p".to_string(), values);
    LogicalInterest {
        id: InterestId(id),
        scope,
        shape: InterestShape {
            kinds: [9735u32].into_iter().collect(),
            tags,
            limit: Some(50),
            p_tag_routing: routing,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle,
    }
}

fn self_zap_receipts_interest() -> LogicalInterest {
    p_tag_interest(
        1,
        &["self"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::Tailing,
        InterestScope::Global,
    )
}

// ── PD-033-C — bootstrap inbox lane (§4.3 — Stage 2 precursor) ──────────

/// Headline routing decision: a `Tailing + Global + #p (Nip65ReadRelays)`
/// interest whose tagged pubkey has NO cached NIP-65 inbox AND
/// `bootstrap_content_relays` is non-empty routes to the bootstrap content
/// lane (lane `UserConfigured(Bootstrap)`). This is the silent-loss
/// regression Stage 2 exposes for the kernel's self-zap-receipts subscription
/// — without this gate, deleting the M1 `req(Content, …)` helper would lose
/// every #p-tagged Tailing REQ until kind:10002 lands.
#[test]
fn pd033c_p_tag_tailing_global_no_inbox_routes_to_bootstrap_content() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_content = vec!["wss://relay.primal.net".to_string()];
    // Active-account / app / indexer relays present to prove the gate
    // chooses BOOTSTRAP specifically, not any of those (all wrong for the
    // self-zap-receipts cold-start: indexer is discovery-only, AccountRead
    // is for hashtag firehose, AppRelay rides Case A not Case C).
    let indexer = vec!["wss://purplepag.es".to_string()];
    let aar = vec!["wss://user-read.example".to_string()];
    let app = vec!["wss://user-app.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &aar,
        &app,
        &bootstrap_content,
        /* bootstrap_indexer = */ &[],
    );

    let plan = compiler
        .compile(&[self_zap_receipts_interest()])
        .expect("compile");

    let landed = plan
        .per_relay
        .get("wss://relay.primal.net")
        .expect("bootstrap content relay must carry the #p Tailing REQ");
    assert!(
        landed.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::Bootstrap
        )),
        "bootstrap content lane must be recorded; got role_tags = {:?}",
        landed.role_tags
    );
    // Exactly one relay served the REQ — none of the other configured
    // relays may carry it (the gate is exclusive, not additive).
    assert_eq!(plan.per_relay.len(), 1);
    assert!(plan.per_relay.get("wss://purplepag.es").is_none());
    assert!(plan.per_relay.get("wss://user-read.example").is_none());
    assert!(plan.per_relay.get("wss://user-app.example").is_none());
}

/// Once kind:10002 arrives for the tagged pubkey, the next recompile re-
/// routes off the bootstrap content lane onto the real inbox relays. This
/// is the load-bearing transition that proves the gate is dynamic — a
/// stuck-on-bootstrap regression would cap zap-receipt delivery to the
/// cold-start lane forever.
#[test]
fn pd033c_p_tag_routes_off_bootstrap_when_inbox_arrives() {
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];

    // Phase 1: no inbox cached → bootstrap.
    let empty_cache = InMemoryMailboxCache::new();
    let before = SubscriptionCompiler::with_relays_and_bootstrap(
        &empty_cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    )
    .compile(&[self_zap_receipts_interest()])
    .expect("compile");
    assert!(
        before.per_relay.contains_key("wss://bootstrap.example"),
        "phase 1: bootstrap carries the #p REQ when no inbox cached"
    );

    // Phase 2: kind:10002 arrives → the same interest re-routes to the
    // real inbox relay and the bootstrap lane is no longer used.
    let mut after_cache = InMemoryMailboxCache::new();
    after_cache.put(
        pk("self"),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec!["wss://self-read.example".to_string()],
            both_relays: vec![],
        },
    );
    let after = SubscriptionCompiler::with_relays_and_bootstrap(
        &after_cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    )
    .compile(&[self_zap_receipts_interest()])
    .expect("compile");
    assert!(
        after.per_relay.contains_key("wss://self-read.example"),
        "phase 2: real inbox carries the #p REQ once kind:10002 lands"
    );
    assert!(
        after.per_relay.get("wss://bootstrap.example").is_none(),
        "phase 2: bootstrap lane MUST be retired when an inbox is cached \
         (gate evaluates false)"
    );
}

/// Counterpoint — empty `bootstrap_content_relays` falls through to the
/// existing Case C body (fail-closed) and emits ZERO relay entries. Proves
/// the gate is a strict superset opt-in.
#[test]
fn pd033c_p_tag_empty_bootstrap_falls_through_to_fail_closed() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        /* bootstrap_content = */ &[],
        &[],
    );

    let plan = compiler
        .compile(&[self_zap_receipts_interest()])
        .expect("compile");

    assert!(
        plan.per_relay.is_empty(),
        "Case C fail-closed semantics preserved when bootstrap is empty; \
         got per_relay = {:?}",
        plan.per_relay.keys().collect::<Vec<_>>()
    );
}

/// Counterpoint — `OneShot + Global + #p` does NOT trigger the gate. The
/// gate is keyed on Tailing specifically (the self-zap-receipts shape;
/// OneShot would imply a one-time inbox probe, which is not a defined
/// kernel path today). A future OneShot+#p caller would need its own
/// explicit gate.
#[test]
fn pd033c_p_tag_oneshot_does_not_trigger_gate() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    let interest = p_tag_interest(
        1,
        &["self"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::OneShot,
        InterestScope::Global,
    );

    let plan = compiler.compile(&[interest]).expect("compile");
    assert!(
        plan.per_relay.get("wss://bootstrap.example").is_none(),
        "OneShot + #p must NOT trigger the bootstrap inbox gate (the gate \
         is scoped to the Tailing self-zap-receipts shape)"
    );
    // Falls through to the regular Case C path which fail-closes when
    // inbox is unknown.
    assert!(plan.per_relay.is_empty());
}

/// Counterpoint — `Tailing + Account(x) + #p` does NOT trigger the gate.
/// Account-scoped #p interests have an explicit account context and should
/// route via that account's inbox (or fail-closed) — diverting them to a
/// shared cold-start lane would mix multi-account contexts on one relay.
#[test]
fn pd033c_p_tag_account_scoped_does_not_trigger_gate() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    let interest = p_tag_interest(
        1,
        &["self"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::Tailing,
        InterestScope::Account(pk("alice")),
    );

    let plan = compiler.compile(&[interest]).expect("compile");
    assert!(
        plan.per_relay.get("wss://bootstrap.example").is_none(),
        "Account-scoped #p must NOT divert to the bootstrap content lane"
    );
}

/// Counterpoint — `Tailing + Global + #p (Nip17DmRelays)` MUST stay
/// fail-closed. NIP-17 gift-wrapped DMs are private; diverting them to a
/// non-DM relay would leak gift-wraps to a relay the recipient never
/// authorised. This counterpoint locks the privacy-critical exclusion.
#[test]
fn pd033c_p_tag_nip17_dm_routing_stays_fail_closed() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    let interest = p_tag_interest(
        1,
        &["self"],
        PTagRouting::Nip17DmRelays,
        InterestLifecycle::Tailing,
        InterestScope::Global,
    );

    let plan = compiler.compile(&[interest]).expect("compile");
    assert!(
        plan.per_relay.is_empty(),
        "NIP-17 DM routing MUST stay fail-closed when DM relays are \
         unknown — diverting gift-wraps to a non-DM relay would leak \
         private DMs. Got per_relay = {:?}",
        plan.per_relay.keys().collect::<Vec<_>>()
    );
}

/// Counterpoint — when ANY tagged pubkey has a cached NIP-65 inbox, the
/// gate refuses and the regular Case C body fires for all pubkeys. The
/// bootstrap fallback must NOT additively double-route (would
/// over-subscribe the bootstrap relay).
#[test]
fn pd033c_p_tag_partial_inbox_cache_does_not_trigger_gate() {
    let mut cache = InMemoryMailboxCache::new();
    // Bob has a cached inbox; Carol does not.
    cache.put(
        pk("bob"),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec!["wss://bob-read.example".to_string()],
            both_relays: vec![],
        },
    );
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    let interest = p_tag_interest(
        1,
        &["bob", "carol"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::Tailing,
        InterestScope::Global,
    );

    let plan = compiler.compile(&[interest]).expect("compile");
    // Bob's inbox carries his #p shard via the regular Case C body.
    assert!(
        plan.per_relay.get("wss://bob-read.example").is_some(),
        "Bob's NIP-65 inbox must carry his #p shard via the regular \
         Case C body"
    );
    // Bootstrap content MUST NOT be touched — partial cache disables
    // the gate so the regular fail-closed semantics apply to Carol.
    assert!(
        plan.per_relay.get("wss://bootstrap.example").is_none(),
        "partial inbox cache must DISABLE the bootstrap fallback (the \
         gate is all-or-nothing); got per_relay = {:?}",
        plan.per_relay.keys().collect::<Vec<_>>()
    );
}

/// Counterpoint — a tagged pubkey with a cached snapshot whose
/// `has_inbox_relays()` returns `false` (an empty kind:10002 declared
/// zero read relays) IS treated as "no inbox" by the gate, exactly as
/// the "no snapshot at all" case. Pins the predicate semantics.
#[test]
fn pd033c_p_tag_empty_inbox_snapshot_treated_as_no_inbox() {
    let mut cache = InMemoryMailboxCache::new();
    // An author whose kind:10002 declared write relays but zero read
    // relays. Per NIP-65 the snapshot exists but `has_inbox_relays()` is
    // false. The gate must treat this as "no inbox" and divert.
    cache.put(
        pk("self"),
        MailboxSnapshot {
            write_relays: vec!["wss://self-write.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    let plan = compiler
        .compile(&[self_zap_receipts_interest()])
        .expect("compile");
    assert!(
        plan.per_relay.contains_key("wss://bootstrap.example"),
        "an empty inbox snapshot must be treated as no-inbox by the gate"
    );
}

/// `bootstrap_content_relays` MUST be excluded from `compute_plan_id` —
/// toggling it at runtime must not churn sub-ids (matches the
/// `app_relays` treatment and the existing Case D bootstrap test
/// `pd033c_bootstrap_toggle_does_not_change_plan_id`). Without this,
/// every kind:10002 arrival would invalidate every plan-id and trigger a
/// spurious re-emit of every wire frame.
#[test]
fn pd033c_p_tag_bootstrap_toggle_does_not_change_plan_id() {
    let cache = InMemoryMailboxCache::new();
    let interests = [self_zap_receipts_interest()];

    let bootstrap_set = vec!["wss://bootstrap.example".to_string()];
    let no_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        /* bootstrap_content = */ &[],
        &[],
    );
    let with_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_set,
        &[],
    );

    let plan_without = no_bootstrap.compile(&interests).expect("compile");
    let plan_with = with_bootstrap.compile(&interests).expect("compile");

    // Behaviour differs — without bootstrap, fail-closed; with bootstrap,
    // routed to the cold-start lane.
    assert!(plan_without.per_relay.is_empty());
    assert!(plan_with.per_relay.contains_key("wss://bootstrap.example"));

    // But plan_id is identical — bootstrap_content_relays is excluded
    // from the hash, matching the app_relays-toggle invariant.
    assert_eq!(
        plan_without.plan_id, plan_with.plan_id,
        "bootstrap_content_relays must be excluded from compute_plan_id; \
         toggling it MUST NOT churn sub-ids"
    );
}
