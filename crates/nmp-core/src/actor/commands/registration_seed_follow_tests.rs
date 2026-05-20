//! Registration seed-follow invariants.
//!
//! New local accounts must start with the product seed follows already present
//! in Rust-owned state. The app shell should not need to open author feeds
//! after onboarding; the subscription lifecycle receives the
//! follow-feed interests and emits the outbox-routed REQs.

use super::*;
use crate::kernel::Kernel;
use crate::planner::InterestLifecycle;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const SEED_NPUB_HEX: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const FIATJAF_HEX: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

fn fresh() -> (IdentityRuntime, Kernel) {
    (IdentityRuntime::new(), Kernel::new(DEFAULT_VISIBLE_LIMIT))
}

fn onboarding_relays() -> Vec<(String, String)> {
    vec![
        (
            "wss://onboard-write.relay/".to_string(),
            "write".to_string(),
        ),
        ("wss://onboard-read.relay/".to_string(), "read".to_string()),
    ]
}

fn event_jsons_of_kind(outbound: &[crate::relay::OutboundMessage], kind: u64) -> Vec<Value> {
    outbound
        .iter()
        .filter(|m| m.text.starts_with("[\"EVENT\""))
        .filter_map(|m| {
            let parsed = serde_json::from_str::<Value>(&m.text).ok()?;
            let event = parsed.as_array()?.get(1)?.clone();
            (event.get("kind").and_then(Value::as_u64) == Some(kind)).then_some(event)
        })
        .collect()
}

fn p_tag_values(event: &Value) -> BTreeSet<String> {
    event
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| tag.as_array())
        .filter(|tag| tag.first().and_then(Value::as_str) == Some("p"))
        .filter_map(|tag| tag.get(1).and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn reqs_by_relay(frames: &[WireFrame]) -> BTreeMap<String, Vec<(&str, &InterestLifecycle)>> {
    let mut reqs = BTreeMap::new();
    for frame in frames {
        if let WireFrame::Req {
            relay_url,
            filter_json,
            lifecycle,
            ..
        } = frame
        {
            reqs.entry(relay_url.clone())
                .or_insert_with(Vec::new)
                .push((filter_json.as_str(), lifecycle));
        }
    }
    reqs
}

#[test]
fn create_account_installs_exact_default_followfeed_and_self() {
    let (mut identity, mut kernel) = fresh();
    let profile = HashMap::new();
    let outbound = create_account(
        &mut identity,
        &mut kernel,
        false,
        &profile,
        &onboarding_relays(),
    );
    let active = identity.active_pubkey().expect("new account pubkey");

    let authors = kernel.timeline_authors_for_test();
    assert!(authors.contains(SEED_NPUB_HEX));
    assert!(authors.contains(FIATJAF_HEX));
    assert!(authors.contains(&active));
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "new account must install one follow-feed interest per seed follow plus self"
    );

    let kind3 = event_jsons_of_kind(&outbound, 3)
        .pop()
        .expect("create_account must publish the seed kind:3 contacts event");
    assert_eq!(
        p_tag_values(&kind3),
        [SEED_NPUB_HEX.to_string(), FIATJAF_HEX.to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn create_account_followfeed_discovers_relays_and_keeps_reqs_tailing() {
    let (mut identity, mut kernel) = fresh();
    let profile = HashMap::new();
    create_account(
        &mut identity,
        &mut kernel,
        false,
        &profile,
        &onboarding_relays(),
    );
    let active = identity.active_pubkey().expect("new account pubkey");

    kernel.seed_kind10002_for_test(SEED_NPUB_HEX, &["wss://seed-follow.relay/"]);
    kernel.seed_kind10002_for_test(FIATJAF_HEX, &["wss://fiatjaf-follow.relay/"]);
    kernel.seed_kind10002_for_test(&active, &["wss://self-follow.relay/"]);
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    let frames = kernel.drain_lifecycle_tick();
    let reqs = reqs_by_relay(&frames);
    for relay in [
        "wss://seed-follow.relay/",
        "wss://fiatjaf-follow.relay/",
        "wss://self-follow.relay/",
    ] {
        let frames_for_relay = reqs
            .get(relay)
            .unwrap_or_else(|| panic!("missing tailing REQ for {relay}; frames={frames:?}"));
        assert!(
            frames_for_relay
                .iter()
                .any(|(_, lifecycle)| matches!(lifecycle, InterestLifecycle::Tailing)),
            "follow-feed REQ for {relay} must stay open"
        );
        let filter = frames_for_relay[0].0;
        let json = serde_json::from_str::<Value>(filter).expect("REQ filter JSON");
        let kinds = json
            .get("kinds")
            .and_then(Value::as_array)
            .expect("follow feed filter must carry kinds");
        assert!(kinds.contains(&Value::from(1)));
        assert!(kinds.contains(&Value::from(6)));
        assert_eq!(json.get("limit"), Some(&Value::from(200)));
    }
}
