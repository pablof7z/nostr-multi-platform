//! PD-033-C — `set_bootstrap_content_relays` / `set_bootstrap_indexer_relays`
//! end-to-end wiring, plus the remaining `lifecycle.rs` setter/accessor
//! round-trips: `set_indexer_relays` replace-not-append semantics, the
//! `last_planner_error` test seam, and `clear_probed_mailboxes`.
use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest,
};

/// PD-033-C planner extension end-to-end smoke: setting bootstrap content
/// relays on the lifecycle and registering a `OneShot + Global + event_ids`
/// discovery interest produces a `WireFrame::Req` addressed to the bootstrap
/// URL. Proves the lifecycle threads the new field into the compiler and the
/// compiler's new gate fires through to the wire-emitter.
///
/// This is the integration counterpart to the in-tree
/// `case_d_no_author::tests::pd033c_event_ids_oneshot_global_routes_to_bootstrap_content`
/// unit test — together they pin the planner-side prerequisite for Stage 1.
#[test]
fn pd033c_bootstrap_content_relays_threaded_into_recompile() {
    let mut l = SubscriptionLifecycle::new();
    // Drop the cfg(test) placeholder default so we can prove the discovery
    // REQ lands on bootstrap content, not the indexer fallback.
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_content_relays(vec!["wss://content-relay.example".to_string()]);

    let mailboxes = InMemoryMailboxCache::new();
    // Discovery oneshot for an event id — matches `oneshot.request(...)` in
    // `kernel/discovery.rs::drain_unknown_oneshots` (events arm).
    let event_id_hex: String = "aa".repeat(32);
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            event_ids: [event_id_hex.clone()].into_iter().collect(),
            limit: Some(1),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };
    push_legacy(l.registry_mut(), interest);

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let landed: Vec<&WireFrame> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://content-relay.example"))
        .collect();
    assert_eq!(
        landed.len(),
        1,
        "exactly one REQ must land on the bootstrap content relay; got {} frames in total: {:?}",
        landed.len(),
        frames
    );
    if let WireFrame::Req {
        lifecycle,
        filter_json,
        ..
    } = landed[0]
    {
        assert!(
            matches!(lifecycle, InterestLifecycle::OneShot),
            "the bootstrap REQ must carry OneShot lifecycle (CLOSE on EOSE)"
        );
        assert!(
            filter_json.contains(&event_id_hex),
            "the bootstrap REQ filter must carry the discovery event_id; got {filter_json}"
        );
    } else {
        panic!("expected a WireFrame::Req on the bootstrap relay");
    }
}

/// `set_bootstrap_content_relays` REPLACES the bootstrap set wholesale —
/// matches the `set_indexer_relays` / `set_app_relays` setter contract. An
/// empty Vec disables the bootstrap gate, falling back to the unchanged
/// Case D body.
#[test]
fn pd033c_set_bootstrap_content_relays_replaces_rather_than_appends() {
    let mut l = SubscriptionLifecycle::new();
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_content_relays(vec!["wss://first.example".to_string()]);
    l.set_bootstrap_content_relays(vec![
        "wss://second.example".to_string(),
        "wss://third.example".to_string(),
    ]);

    let mailboxes = InMemoryMailboxCache::new();
    let event_id_hex: String = "bb".repeat(32);
    push_legacy(
        l.registry_mut(),
        LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                event_ids: [event_id_hex].into_iter().collect(),
                limit: Some(1),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: false,
        },
    );

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let urls: std::collections::BTreeSet<String> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert!(
        urls.contains("wss://second.example") && urls.contains("wss://third.example"),
        "later setter call must REPLACE the prior set; got {urls:?}"
    );
    assert!(
        !urls.contains("wss://first.example"),
        "the first bootstrap URL must have been replaced, not retained; got {urls:?}"
    );
}

/// PD-033-C end-to-end smoke for the profile-oneshot arm: setting
/// `bootstrap_indexer_relays` on the lifecycle and registering a `OneShot +
/// Global + authors`-shaped profile-fetch interest (no NIP-65 mailbox, no
/// app_relays) produces a `WireFrame::Req` addressed to the bootstrap indexer.
/// Mirrors `kernel/discovery.rs::drain_unknown_oneshots`'s profile-oneshot
/// fan-out — the planner-side parity check Stage 1 depends on.
#[test]
fn pd033c_bootstrap_indexer_relays_threaded_into_recompile() {
    let mut l = SubscriptionLifecycle::new();
    // Drop the cfg(test) raw indexer default so we can prove the discovery
    // REQ lands on the BOOTSTRAP indexer specifically (not the raw one — the
    // cold-start divergence the planner extension fixes).
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_indexer_relays(vec!["wss://indexer-relay.example".to_string()]);

    let mailboxes = InMemoryMailboxCache::new();
    // Profile-shape oneshot — matches `oneshot.request(...)` in
    // `kernel/discovery.rs::drain_unknown_oneshots` (profiles arm).
    let bob: String = "ab".repeat(32);
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [bob.clone()].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        // The planner-extension bootstrap-indexer fallback gate is now the
        // explicit `is_indexer_discovery` flag (was: OneShot + Global). The
        // discovery-direction profile-shape interest opts in.
        is_indexer_discovery: true,
    };
    push_legacy(l.registry_mut(), interest);

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let landed: Vec<&WireFrame> = frames
        .iter()
        .filter(
            |f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://indexer-relay.example"),
        )
        .filter(|f| match f {
            // Discriminate the bootstrap-indexer profile fetch from any
            // mailbox-probe REQ that might also land on the same URL — the
            // probe is a separate auxiliary frame and its sub_id is prefixed.
            WireFrame::Req { sub_id, .. } => !sub_id.starts_with("mailbox-probe-"),
            _ => false,
        })
        .collect();
    assert_eq!(
        landed.len(),
        1,
        "exactly one profile-fetch REQ must land on the bootstrap indexer; \
         got {} matching frames in {} total",
        landed.len(),
        frames.len(),
    );
    if let WireFrame::Req {
        lifecycle,
        filter_json,
        ..
    } = landed[0]
    {
        assert!(
            matches!(lifecycle, InterestLifecycle::OneShot),
            "the bootstrap-indexer REQ must carry OneShot lifecycle"
        );
        assert!(
            filter_json.contains(&bob),
            "the bootstrap-indexer REQ filter must carry the discovery author; got {filter_json}"
        );
    } else {
        panic!("expected a WireFrame::Req on the bootstrap indexer");
    }
    // PD-033-C invariant: the discovery author MUST NOT be unroutable.
    assert!(
        !l.current_plan_unroutable().contains(&bob),
        "PD-033-C invariant: the discovery-oneshot author must NOT be unroutable"
    );
}

/// `set_indexer_relays` REPLACES the indexer set wholesale — it does not
/// append to the `#[cfg(test)]` placeholder default. Setting an empty Vec
/// disables the indexer fallback entirely.
#[test]
fn set_indexer_relays_replaces_rather_than_appends() {
    let mut l = SubscriptionLifecycle::new();
    // cfg(test) default is the single placeholder entry.
    assert_eq!(l.indexer_relays().len(), 1);

    l.set_indexer_relays(vec![
        "wss://relay.one".to_string(),
        "wss://relay.two".to_string(),
    ]);
    assert_eq!(
        l.indexer_relays(),
        ["wss://relay.one".to_string(), "wss://relay.two".to_string()].as_slice(),
        "set_indexer_relays must replace the default, not append to it",
    );

    l.set_indexer_relays(Vec::new());
    assert!(
        l.indexer_relays().is_empty(),
        "an empty Vec must disable the indexer fallback",
    );
}

/// `last_planner_error` round-trips through the `#[cfg(test)]`
/// `set_planner_error_for_test` seam: `None` at construction, then the
/// injected string, with latest-error-wins semantics on a second injection.
#[test]
fn last_planner_error_round_trips_through_test_seam() {
    let mut l = SubscriptionLifecycle::new();
    assert!(l.last_planner_error().is_none(), "no error at construction");

    l.set_planner_error_for_test("InvalidShape: empty kind set");
    assert_eq!(
        l.last_planner_error(),
        Some("InvalidShape: empty kind set"),
        "injected error must be observable",
    );

    l.set_planner_error_for_test("HashingFailed");
    assert_eq!(
        l.last_planner_error(),
        Some("HashingFailed"),
        "latest-error-wins: the second injection must overwrite the first",
    );
}

/// `clear_probed_mailboxes` empties the implicit-discovery probed set — the
/// `refresh` escape hatch that forces every still-unknown author to be
/// re-probed on the next recompile. The set is seeded directly via the
/// private field (no public setter exists; descendant-module access applies).
#[test]
fn clear_probed_mailboxes_empties_the_probed_set() {
    let mut l = SubscriptionLifecycle::new();
    l.probed_mailboxes.insert(pubkey("a"));
    l.probed_mailboxes.insert(pubkey("b"));
    assert_eq!(l.probed_mailboxes().len(), 2, "probed set seeded with 2");

    l.clear_probed_mailboxes();

    assert!(
        l.probed_mailboxes().is_empty(),
        "clear_probed_mailboxes must empty the set so authors are re-probed",
    );
}
