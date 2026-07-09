use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestScope, MailboxSnapshot, NaddrCoord,
    SubscriptionCompiler,
};

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// A syntactically valid 64-char hex pubkey (unlike `pubkey()` above, whose
/// zero-padded fixtures are NOT valid hex whenever `s` contains non-hex
/// letters). `filter_json_for`'s `addresses` serialisation round-trips a
/// coordinate through `nostr::PublicKey::from_hex`, so tests that need the
/// coordinate to actually survive onto the wire need a real hex string.
fn hex_pubkey(byte: &str) -> String {
    byte.repeat(32)
}

fn ti(id: u64, authors: &[&str], lc: InterestLifecycle) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pubkey(a)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: lc,
        is_indexer_discovery: false,
    }
}

// ── F-CROSS-1: relay-scoped diff keying ─────────────────────────────────

fn snap(write_relays: Vec<&str>) -> MailboxSnapshot {
    MailboxSnapshot {
        write_relays: write_relays.into_iter().map(str::to_string).collect(),
        read_relays: vec![],
        both_relays: vec![],
    }
}

#[test]
fn filter_json_preserves_search_field() {
    let shape = InterestShape {
        kinds: [1u32].into_iter().collect(),
        search: Some("nostr rust".to_string()),
        ..Default::default()
    };

    let json: serde_json::Value =
        serde_json::from_str(&filter_json_for(&shape)).expect("filter json");
    assert_eq!(
        json.get("search").and_then(|v| v.as_str()),
        Some("nostr rust")
    );
}

// ── #3091: `addresses` round-trips through `filter_json_for` → `from_filter_json` ──

/// A non-empty `shape.addresses` must survive the wire round-trip losslessly,
/// landing back in `shape.addresses` — NOT the opaque `tags["a"]` bucket the
/// generic single-letter-tag branch used to swallow it into (#3091). This is
/// the exact inverse pairing `filter_json_for` (serialize) exercises against
/// `InterestShape::from_filter_json` (parse).
#[test]
fn addresses_round_trip_filter_json_for_and_from_filter_json() {
    let coord = NaddrCoord {
        pubkey: hex_pubkey("aa"),
        kind: 30_023,
        d_tag: "my-article".to_string(),
    };
    let shape = InterestShape {
        addresses: [coord.clone()].into_iter().collect(),
        ..Default::default()
    };

    let json = filter_json_for(&shape);
    let round_tripped = InterestShape::from_filter_json(&json).expect("valid filter json");

    assert_eq!(
        round_tripped.addresses,
        [coord].into_iter().collect(),
        "the coordinate must land back in `addresses`, byte-identical"
    );
    assert!(
        round_tripped.tags.is_empty(),
        "the #a tag must NOT fall through into the generic `tags` bucket"
    );
}

/// Same round-trip, but for a non-parameterized-replaceable coordinate (empty
/// `d_tag`, e.g. kind:10002 NIP-65 relay list) — `Coordinate::Display` still
/// emits the trailing empty identifier segment (`"10002:<pubkey>:"`), and the
/// parser must decode that back into `d_tag: ""`, not drop the coordinate.
#[test]
fn addresses_round_trip_preserves_empty_d_tag() {
    let coord = NaddrCoord {
        pubkey: hex_pubkey("bb"),
        kind: 10_002,
        d_tag: String::new(),
    };
    let shape = InterestShape {
        addresses: [coord.clone()].into_iter().collect(),
        ..Default::default()
    };

    let json = filter_json_for(&shape);
    let round_tripped = InterestShape::from_filter_json(&json).expect("valid filter json");

    assert_eq!(round_tripped.addresses, [coord].into_iter().collect());
}

fn req_relays(frames: &[WireFrame]) -> std::collections::BTreeSet<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

fn close_relays(frames: &[WireFrame]) -> std::collections::BTreeSet<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

/// One author → two write relays (same filter hash). Both relays must
/// receive a REQ on the initial diff (prior = None).
#[test]
fn plan_diff_overlapping_filter_two_relays_emits_per_relay_frames() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("overlap_a"),
        snap(vec!["wss://relay-x.example", "wss://relay-y.example"]),
    );
    let interests = vec![ti(1, &["overlap_a"], InterestLifecycle::Tailing)];
    let plan = SubscriptionCompiler::new(&cache, &[])
        .compile(&interests)
        .expect("compile");
    assert!(plan.per_relay.len() >= 2, "need both relays in plan");
    let frames = plan_diff(None, Some(&plan), &interests);
    let reqs = req_relays(&frames);
    assert!(
        reqs.contains("wss://relay-x.example"),
        "relay-x must get REQ; {reqs:?}"
    );
    assert!(
        reqs.contains("wss://relay-y.example"),
        "relay-y must get REQ; {reqs:?}"
    );
}

/// Same filter on two relays. One relay removed in next plan.
/// CLOSE must be emitted for the removed relay only, not for the survivor.
/// Fails on current code: surviving relay still contributes the same sub_id
/// to the global next-set → no CLOSE emitted for the removed relay.
#[test]
fn plan_diff_dead_relay_with_shared_filter_emits_close() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("dead_b"),
        snap(vec![
            "wss://relay-alive.example",
            "wss://relay-dead.example",
        ]),
    );
    let interests = vec![ti(1, &["dead_b"], InterestLifecycle::Tailing)];
    let prior_plan = SubscriptionCompiler::new(&cache, &[])
        .compile(&interests)
        .expect("prior");
    assert!(prior_plan
        .per_relay
        .contains_key("wss://relay-alive.example"));
    assert!(prior_plan
        .per_relay
        .contains_key("wss://relay-dead.example"));

    let mut cache2 = InMemoryMailboxCache::new();
    cache2.put(pubkey("dead_b"), snap(vec!["wss://relay-alive.example"]));
    let next_plan = SubscriptionCompiler::new(&cache2, &[])
        .compile(&interests)
        .expect("next");

    let closes = close_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests));
    assert!(
        closes.contains("wss://relay-dead.example"),
        "CLOSE for dead relay; {closes:?}"
    );
    assert!(
        !closes.contains("wss://relay-alive.example"),
        "no CLOSE for alive relay; {closes:?}"
    );
}

/// Author already on NIP-65 relay X. App relay Y added in next plan.
/// Y must receive a REQ even though it carries the same filter hash as X.
/// Fails on current code: sub_id already present in prior global set → REQ skipped.
#[test]
fn plan_diff_app_relay_add_for_already_routed_author_emits_req() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pubkey("app_a"), snap(vec!["wss://relay-nip65.example"]));
    let interests = vec![ti(1, &["app_a"], InterestLifecycle::Tailing)];
    let prior_plan = SubscriptionCompiler::new(&cache, &[])
        .compile(&interests)
        .expect("prior");
    assert!(prior_plan
        .per_relay
        .contains_key("wss://relay-nip65.example"));

    let app_relays = vec!["wss://app-relay-y.example".to_string()];
    let next_plan = SubscriptionCompiler::with_relays(&cache, &[], &[], &app_relays)
        .compile(&interests)
        .expect("next");
    assert!(
        next_plan
            .per_relay
            .contains_key("wss://app-relay-y.example"),
        "next plan must include app relay; got {:?}",
        next_plan.per_relay.keys().collect::<Vec<_>>()
    );

    let reqs = req_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests));
    assert!(
        reqs.contains("wss://app-relay-y.example"),
        "app relay Y must get REQ; {reqs:?}"
    );
    assert!(
        !reqs.contains("wss://relay-nip65.example"),
        "NIP-65 relay X must not get redundant REQ; {reqs:?}"
    );
}

/// Regression: unique (author, relay) pairs still behave correctly with relay-scoped keying.
/// Two authors, each with a unique write relay. Drop one → CLOSE on that relay only.
#[test]
fn plan_diff_unique_pairs_regression_still_works() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pubkey("unique_a"), snap(vec!["wss://unique-r1.example"]));
    cache.put(pubkey("unique_b"), snap(vec!["wss://unique-r2.example"]));
    let interests = vec![
        ti(1, &["unique_a"], InterestLifecycle::Tailing),
        ti(2, &["unique_b"], InterestLifecycle::Tailing),
    ];
    let prior_plan = SubscriptionCompiler::new(&cache, &[])
        .compile(&interests)
        .expect("prior");
    let first_reqs = req_relays(&plan_diff(None, Some(&prior_plan), &interests));
    assert!(first_reqs.contains("wss://unique-r1.example"), "r1 REQ");
    assert!(first_reqs.contains("wss://unique-r2.example"), "r2 REQ");

    let mut cache2 = InMemoryMailboxCache::new();
    cache2.put(pubkey("unique_a"), snap(vec!["wss://unique-r1.example"]));
    let interests2 = vec![ti(1, &["unique_a"], InterestLifecycle::Tailing)];
    let next_plan = SubscriptionCompiler::new(&cache2, &[])
        .compile(&interests2)
        .expect("next");
    let closes = close_relays(&plan_diff(Some(&prior_plan), Some(&next_plan), &interests2));
    assert!(
        closes.contains("wss://unique-r2.example"),
        "r2 CLOSE; {closes:?}"
    );
    assert!(
        !closes.contains("wss://unique-r1.example"),
        "no r1 CLOSE; {closes:?}"
    );
}

// ── existing tests ───────────────────────────────────────────────────────

#[test]
fn diff_against_empty_emits_all_reqs() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("a"),
        MailboxSnapshot {
            write_relays: vec!["wss://r1".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let indexer = vec!["wss://ix".to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let interests = vec![ti(1, &["a"], InterestLifecycle::Tailing)];
    let plan = compiler.compile(&interests).expect("compile");

    let frames = plan_diff(None, Some(&plan), &interests);
    let reqs = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    let closes = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count();
    assert!(reqs >= 1);
    assert_eq!(closes, 0);
}

#[test]
fn diff_identical_is_empty() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("a"),
        MailboxSnapshot {
            write_relays: vec!["wss://r1".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let indexer = vec!["wss://ix".to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let interests = vec![ti(1, &["a"], InterestLifecycle::Tailing)];
    let plan = compiler.compile(&interests).expect("compile");
    let frames = plan_diff(Some(&plan), Some(&plan), &interests);
    assert!(frames.is_empty(), "identical plans → empty diff");
}
