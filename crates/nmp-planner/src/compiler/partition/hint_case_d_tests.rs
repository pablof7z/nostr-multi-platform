use crate::{
    compiler::{InMemoryMailboxCache, SubscriptionCompiler},
    interest::{
        HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        RelayHint,
    },
    plan::{RoutingSource, UserConfiguredCategory},
};

fn event_id(byte: &str) -> String {
    byte.repeat(32)
}

fn event_id_interest_with_hint(url: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            event_ids: [event_id("aa")].into_iter().collect(),
            limit: Some(1),
            ..Default::default()
        },
        hints: vec![RelayHint {
            url: url.to_string(),
            source: HintSource::EventTag {
                event_id: event_id("bb"),
                tag: "e".to_string(),
                position: 2,
            },
        }],
        lifecycle: InterestLifecycle::OneShot,
    }
}

#[test]
fn case_d_event_id_hint_routes_to_canonical_hint_relay() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap = vec!["wss://bootstrap.example".to_string()];
    let compiler =
        SubscriptionCompiler::with_relays_and_bootstrap(&cache, &[], &[], &[], &bootstrap, &[]);

    let plan = compiler
        .compile(&[event_id_interest_with_hint("WSS://Hint.Example/")])
        .expect("compile");

    let hinted = plan
        .per_relay
        .get("wss://hint.example")
        .expect("hint relay must carry no-author event-id interest");
    assert!(hinted.role_tags.contains(&RoutingSource::Hint));
    let boot = plan
        .per_relay
        .get("wss://bootstrap.example")
        .expect("bootstrap fallback still stacks with event-id hint");
    assert!(boot.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::Bootstrap,
    )));
}
