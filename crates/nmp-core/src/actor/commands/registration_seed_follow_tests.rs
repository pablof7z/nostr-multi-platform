//! Registration seed-follow invariants.
//!
//! New local accounts must start with the product seed follows already present
//! in Rust-owned state. The app shell should not need to open author feeds
//! after onboarding; the subscription lifecycle receives the
//! follow-feed interests and emits the outbox-routed REQs.

use super::*;
use crate::kernel::Kernel;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::{CompileTrigger, InvalidateReason, WireFrame};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const SEED_NPUB_HEX: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const FIATJAF_HEX: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

fn fresh() -> (IdentityRuntime, Kernel) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Declare the host kinds {1, 6} the contact-feed subscription REQs for,
    // as the FFI `nmp_app_chirp_open_home_feed` does in production. Without
    // this the kernel's `follow_feed_kinds` is empty and `sync_follow_feed_interests`
    // registers nothing (D0: the substrate no longer hardcodes a kind set).
    kernel.follow_feed_kinds = BTreeSet::from([1u32, 6u32]);
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        kernel,
    )
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

fn filter_has_kind_and_authors(filter: &str, kind: u64, authors: &[&str]) -> bool {
    let Ok(json) = serde_json::from_str::<Value>(filter) else {
        return false;
    };
    let has_kind = json
        .get("kinds")
        .and_then(Value::as_array)
        .is_some_and(|kinds| kinds.contains(&Value::from(kind)));
    let has_authors = json
        .get("authors")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            authors
                .iter()
                .all(|author| values.contains(&Value::from(*author)))
        });
    has_kind && has_authors
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
        false,
        true,
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
fn create_account_followfeed_uses_configured_relay_before_mailboxes_arrive() {
    let (mut identity, mut kernel) = fresh();
    let profile = HashMap::new();
    create_account(
        &mut identity,
        &mut kernel,
        false,
        &profile,
        &onboarding_relays(),
        false,
        true,
    );
    let active = identity.active_pubkey().expect("new account pubkey");

    let frames = kernel.drain_lifecycle_tick();
    let reqs = reqs_by_relay(&frames);
    let read_relay = "wss://onboard-read.relay";
    let frames_for_read = reqs
        .get(read_relay)
        .unwrap_or_else(|| panic!("missing app-relay follow-feed REQ; frames={frames:?}"));
    for author in [SEED_NPUB_HEX, FIATJAF_HEX, &active] {
        assert!(
            frames_for_read.iter().any(|(filter, lifecycle)| {
                matches!(lifecycle, InterestLifecycle::Tailing)
                    && filter_has_kind_and_authors(filter, 1, &[author])
                    && filter_has_kind_and_authors(filter, 6, &[author])
            }),
            "fresh account follow-feed must ride configured read relays before \
             followed authors' kind:10002 mailboxes arrive; author={author}; frames={frames:?}"
        );
    }
    assert!(
        kernel.lifecycle_mut().current_plan_unroutable().is_empty(),
        "configured read relays must keep default follows routable while mailboxes are unknown"
    );
}

#[test]
fn create_account_followfeed_probes_default_follow_mailboxes_via_indexer() {
    let (mut identity, mut kernel) = fresh();
    let profile = HashMap::new();
    let relays = vec![(
        "wss://onboard-indexer.relay/".to_string(),
        "both,indexer".to_string(),
    )];
    create_account(&mut identity, &mut kernel, false, &profile, &relays, false, true);

    let frames = kernel.drain_lifecycle_tick();
    let reqs = reqs_by_relay(&frames);
    let frames_for_indexer = reqs
        .get("wss://onboard-indexer.relay")
        .unwrap_or_else(|| panic!("missing indexer mailbox probe; frames={frames:?}"));
    assert!(
        frames_for_indexer.iter().any(|(filter, lifecycle)| {
            matches!(lifecycle, InterestLifecycle::OneShot)
                && filter_has_kind_and_authors(filter, 10002, &[SEED_NPUB_HEX, FIATJAF_HEX])
        }),
        "fresh account follow-feed must probe default follows' kind:10002 mailboxes; \
         frames={frames:?}"
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
        false,
        true,
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
        // V-04 reactive subs: the active-account NIP-65 write relay
        // (self-follow.relay) now receives BOTH the follow-feed Tailing REQ
        // (kinds 1, 6) AND the bootstrap Tailing self-kinds REQ (kinds
        // 0, 3, 10002, 10000, 10006). Both have `Tailing` lifecycle, so
        // discriminate by kinds: the follow feed is the one carrying
        // `1` AND `6`. The kind:10050 DM relay OneShot also rides this
        // relay; it has OneShot lifecycle and is filtered out by the
        // `Tailing` predicate below.
        let (follow_filter, _) = frames_for_relay
            .iter()
            .find(|(filter, lifecycle)| {
                if !matches!(lifecycle, InterestLifecycle::Tailing) {
                    return false;
                }
                let json: Value = match serde_json::from_str(filter) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let kinds = json.get("kinds").and_then(Value::as_array);
                let has_one = kinds.map_or(false, |k| k.contains(&Value::from(1)));
                let has_six = kinds.map_or(false, |k| k.contains(&Value::from(6)));
                has_one && has_six
            })
            .unwrap_or_else(|| {
                panic!(
                    "follow-feed REQ (Tailing kinds [1,6,…]) for {relay} \
                     must be present; got: {frames_for_relay:?}"
                )
            });
        let json = serde_json::from_str::<Value>(follow_filter).expect("REQ filter JSON");
        let kinds = json
            .get("kinds")
            .and_then(Value::as_array)
            .expect("follow feed filter must carry kinds");
        assert!(kinds.contains(&Value::from(1)));
        assert!(kinds.contains(&Value::from(6)));
        assert_eq!(json.get("limit"), Some(&Value::from(1000)));
    }
}

#[test]
fn create_account_prepopulates_self_relay_list_for_inbox_interests() {
    let (mut identity, mut kernel) = fresh();
    let profile = HashMap::new();
    create_account(
        &mut identity,
        &mut kernel,
        false,
        &profile,
        &onboarding_relays(),
        false,
        true,
    );
    let active = identity.active_pubkey().expect("new account pubkey");

    let mut tags = BTreeMap::new();
    tags.insert("p".to_string(), [active.clone()].into_iter().collect());
    kernel.lifecycle_mut().registry_mut().push(LogicalInterest {
        id: InterestId(9_001),
        scope: InterestScope::Account(active),
        shape: InterestShape {
            kinds: [1059].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    });
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    kernel
        .lifecycle_mut()
        .enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });

    let frames = kernel.drain_lifecycle_tick();
    let reqs = reqs_by_relay(&frames);
    let read_reqs = reqs
        .get("wss://onboard-read.relay")
        .unwrap_or_else(|| panic!("missing self inbox REQ on read relay; frames={frames:?}"));
    assert!(
        read_reqs
            .iter()
            .any(|(filter, lifecycle)| filter.contains("\"#p\"")
                && matches!(lifecycle, InterestLifecycle::Tailing)),
        "self inbox interest should route immediately from locally signed kind:10002",
    );
}
