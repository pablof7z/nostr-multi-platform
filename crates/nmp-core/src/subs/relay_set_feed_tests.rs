use std::collections::BTreeSet;

use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest,
};
use crate::subs::{SubscriptionLifecycle, WireFrame};

const APP_A: &str = "wss://relay-set-a.example";
const APP_B: &str = "wss://relay-set-b.example";
const INDEXER: &str = "wss://indexer.example";

fn relay_set_longform_feed() -> LogicalInterest {
    LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [30_023u32].into_iter().collect(),
            ..Default::default()
        },
        lifecycle: InterestLifecycle::Tailing,
        ..Default::default()
    }
}

#[test]
fn relay_set_feed_emits_wire_reqs_without_authors_filter() {
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle.set_indexer_relays(vec![INDEXER.to_string()]);
    lifecycle.set_app_relays(vec![APP_A.to_string(), APP_B.to_string()]);
    let interest = relay_set_longform_feed();
    let token = crate::kernel::cache_serve::RegistryWriteToken::for_test();
    let identity = crate::subs::SubIdentity::for_standing_interest(&interest);
    let _ = lifecycle.registry_mut().apply(
        &token,
        crate::kernel::cache_serve::InterestWrite::Replace,
        identity,
        interest,
    );

    let frames = lifecycle
        .recompile_and_diff(&InMemoryMailboxCache::new())
        .expect("relay-set feed interest compiles");

    let reqs = frames
        .iter()
        .filter_map(|frame| match frame {
            WireFrame::Req {
                relay_url,
                filter_json,
                lifecycle,
                ..
            } => Some((relay_url.as_str(), filter_json.as_str(), lifecycle)),
            WireFrame::Close { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reqs.iter()
            .map(|(relay, _, _)| *relay)
            .collect::<BTreeSet<_>>(),
        [APP_A, APP_B].into_iter().collect(),
        "no-author relay-set feeds must route to app relays, not the indexer lane"
    );

    for (relay, filter_json, lifecycle) in reqs {
        assert_eq!(*lifecycle, InterestLifecycle::Tailing, "{relay}");
        let filter = serde_json::from_str::<serde_json::Value>(filter_json)
            .expect("REQ filter must be valid JSON");
        assert_eq!(
            filter.get("kinds"),
            Some(&serde_json::json!([30_023])),
            "{relay} must request only the declared primary kind"
        );
        assert!(
            filter.get("authors").is_none(),
            "{relay} must not gain an authors filter: {filter_json}"
        );
        assert!(
            filter.get("#p").is_none() && filter.get("#a").is_none() && filter.get("#e").is_none(),
            "{relay} must remain a plain relay-set content feed: {filter_json}"
        );
    }
}
