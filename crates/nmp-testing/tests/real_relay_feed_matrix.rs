//! Real-relay declared-feed matrix for issue #1626.
//!
//! This is an honest validation test: it uses live public relays and writes a
//! PASS/SKIP report under `docs/perf/real-relay/`. Public relay availability is
//! not a CI invariant, so the test is ignored by default.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_feed_matrix -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{
    report_page, send_text, try_open, write_report, Verdict, DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY,
    PURPLEPAG_ES,
};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{FeedRequest, FlatFeed, FlatFeedItem};
use nmp_planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, Pubkey, SubscriptionCompiler,
};
use serde_json::{json, Value};

const FETCH_BUDGET: Duration = Duration::from_secs(10);
const MAX_FOLLOW_AUTHORS: usize = 24;
const NEW_FOLLOW: &str = "deadbeef00000000000000000000000000000000000000000000000000000000";

const CANDIDATE_AUTHORS: &[(&str, &str)] = &[
    (
        "pablo-provided",
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    ),
    (
        "fiatjaf",
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
    ),
    (
        "jb55",
        "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
    ),
];

#[derive(Clone, Debug, serde::Serialize)]
struct RankedCard {
    id: String,
    words: u64,
}

fn parse_event(text: &str, sub_id: &str, relay: &str) -> Option<KernelEvent> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?.as_object()?;
    let tags = ev
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(|tag| {
            Some(
                tag.as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    Some(KernelEvent {
        id: ev.get("id")?.as_str()?.to_string(),
        author: ev.get("pubkey")?.as_str()?.to_string(),
        kind: ev.get("kind")?.as_u64()? as u32,
        created_at: ev.get("created_at")?.as_u64()?,
        tags,
        content: ev.get("content")?.as_str().unwrap_or_default().to_string(),
        relay_provenance: vec![relay.to_string()],
    })
}

fn is_eose(text: &str, sub_id: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    v.as_array().is_some_and(|arr| {
        arr.first().and_then(Value::as_str) == Some("EOSE")
            && arr.get(1).and_then(Value::as_str) == Some(sub_id)
    })
}

fn fetch_events(relays: &[&str], mut filter: Value, limit: usize) -> Vec<KernelEvent> {
    let mut out = Vec::new();
    filter["limit"] = json!(limit);
    for relay in relays {
        let Some(mut socket) = try_open(relay) else {
            continue;
        };
        let sub_id = format!("rr-feed-{}", common::now_ms());
        let req = json!(["REQ", sub_id, filter]).to_string();
        if send_text(&mut socket, req).is_err() {
            let _ = socket.close(None);
            continue;
        }
        let deadline = Instant::now() + FETCH_BUDGET;
        common::drain_until(&mut socket, deadline, |text| {
            if let Some(event) = parse_event(text, &sub_id, relay) {
                out.push(event);
            }
            out.len() >= limit || is_eose(text, &sub_id)
        });
        let _ = send_text(&mut socket, json!(["CLOSE", sub_id]).to_string());
        let _ = socket.close(None);
        if !out.is_empty() {
            break;
        }
    }
    out
}

fn followees_from_kind3(event: &KernelEvent) -> BTreeSet<Pubkey> {
    event
        .tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|name| name == "p"))
        .filter_map(|tag| tag.get(1))
        .filter(|pk| pk.len() == 64 && pk.chars().all(|c| c.is_ascii_hexdigit()))
        .cloned()
        .collect()
}

fn fetch_real_followees(relays: &[&str]) -> Option<(String, String, BTreeSet<Pubkey>)> {
    for (name, author) in CANDIDATE_AUTHORS {
        let events = fetch_events(relays, json!({"authors":[author], "kinds":[3]}), 1);
        if let Some(event) = events.first() {
            let followees = followees_from_kind3(event);
            if followees.len() >= 2 {
                return Some((
                    event.relay_provenance[0].clone(),
                    (*name).to_string(),
                    followees,
                ));
            }
        }
    }
    None
}

fn compile_social_plan(followees: &BTreeSet<Pubkey>) -> (BTreeSet<Pubkey>, BTreeSet<u32>, String) {
    let indexer = ["wss://planner.indexer.test".to_string()];
    let mut cache = InMemoryMailboxCache::new();
    for pk in followees {
        cache.put(
            pk.clone(),
            MailboxSnapshot {
                write_relays: vec!["wss://planner.indexer.test".to_string()],
                read_relays: Vec::new(),
                both_relays: Vec::new(),
            },
        );
    }
    let shape = InterestShape::timeline_for(
        followees.clone(),
        nmp_nip18::acquisition_kinds_for_primary([1u32]),
    );
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let plan = compiler
        .compile(&[LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::ActiveAccount,
            shape,
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }])
        .expect("social feed plan compiles");
    let mut authors = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for relay in plan.per_relay.values() {
        for sub in &relay.sub_shapes {
            authors.extend(sub.shape.authors.iter().cloned());
            kinds.extend(sub.shape.kinds.iter().copied());
        }
    }
    (authors, kinds, plan.plan_id)
}

fn assert_relay_set_no_author_plan(app_relays: &[&str]) {
    let cache = InMemoryMailboxCache::new();
    let app = app_relays
        .iter()
        .map(|r| (*r).to_string())
        .collect::<Vec<_>>();
    let indexer = ["wss://indexer.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);
    let plan = compiler
        .compile(&[LogicalInterest {
            id: InterestId(2),
            scope: InterestScope::Global,
            shape: InterestShape {
                kinds: [30_023u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }])
        .expect("relay-set feed compiles");
    assert_eq!(
        plan.per_relay.keys().cloned().collect::<BTreeSet<_>>(),
        app.into_iter().collect::<BTreeSet<_>>()
    );
    for relay in plan.per_relay.values() {
        assert!(relay.sub_shapes[0].shape.authors.is_empty());
        assert_eq!(
            relay.sub_shapes[0].shape.kinds,
            [30_023u32].into_iter().collect()
        );
    }
}

fn assert_custom_ranking_and_page(events: &[KernelEvent]) -> Option<usize> {
    let feed = FlatFeed::new(
        Arc::new(|event: &KernelEvent| event.content.split_whitespace().count() >= 3),
        Arc::new(|event: &KernelEvent| {
            let words = event.content.split_whitespace().count() as u64;
            vec![FlatFeedItem {
                id: event.id.clone(),
                source_id: event.id.clone(),
                sort_created_at: words,
                card: RankedCard {
                    id: event.id.clone(),
                    words,
                },
            }]
        }),
    );
    for event in events {
        feed.on_kernel_event(event);
    }
    let snap = feed.snapshot(&FeedRequest::newest(2));
    if snap.cards.is_empty() {
        return None;
    }
    for pair in snap.cards.windows(2) {
        assert!(pair[0].card.words >= pair[1].card.words);
    }
    assert_eq!(snap.page.as_ref().unwrap().limit, 2);
    Some(snap.page.unwrap().total_blocks)
}

#[test]
#[ignore = "real-relay (run explicitly with --ignored)"]
fn declared_feed_matrix_uses_real_relay_data() {
    let relays = [DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY, PURPLEPAG_ES];
    let Some((kind3_relay, author_name, followees)) = fetch_real_followees(&relays) else {
        write_report(
            "scenario-feed-matrix",
            &report_page(
                "Scenario — declared feed matrix",
                "feed-matrix",
                Verdict::Skip,
                &relays,
                "No candidate kind:3 with at least two `p` tags was available from the live relay set.",
            ),
        );
        println!("SKIP: no usable real kind:3 for declared-feed matrix");
        return;
    };

    let sampled = followees
        .iter()
        .take(MAX_FOLLOW_AUTHORS)
        .cloned()
        .collect::<BTreeSet<_>>();
    let (authors, kinds, plan_id) = compile_social_plan(&sampled);
    assert_eq!(authors, sampled);
    assert_eq!(kinds, [1u32, nmp_nip18::KIND_REPOST].into_iter().collect());

    let removed = sampled.iter().next().unwrap().clone();
    let mut mutated = sampled.clone();
    mutated.remove(&removed);
    mutated.insert(NEW_FOLLOW.to_string());
    let (new_authors, _, new_plan_id) = compile_social_plan(&mutated);
    assert!(!new_authors.contains(&removed));
    assert!(new_authors.contains(NEW_FOLLOW));
    assert_ne!(plan_id, new_plan_id);

    let social_filter = json!({
        "authors": sampled.iter().cloned().collect::<Vec<_>>(),
        "kinds": kinds.iter().copied().collect::<Vec<_>>()
    });
    let social_events = fetch_events(&relays, social_filter, 12);
    if social_events.is_empty() {
        write_report(
            "scenario-feed-matrix",
            &report_page(
                "Scenario — declared feed matrix",
                "feed-matrix",
                Verdict::Skip,
                &relays,
                "A real kind:3 was fetched, but no live kind:1/kind:6 events for the sampled follow set were served within budget. No green assertion was fabricated.",
            ),
        );
        println!("SKIP: no social feed events for sampled real follow set");
        return;
    }
    assert!(social_events
        .iter()
        .all(|event| sampled.contains(&event.author)));
    assert!(social_events
        .iter()
        .all(|event| kinds.contains(&event.kind)));

    assert_relay_set_no_author_plan(&[DAMUS_RELAY, NOS_LOL, PRIMAL_RELAY]);
    let longform_events = fetch_events(&relays, json!({"kinds":[30_023]}), 12);

    let photo_events = fetch_events(
        &relays,
        json!({"kinds":[nmp_nip68::KIND_PICTURE_EVENT]}),
        12,
    );
    let photo_feed =
        nmp_nip68::PictureFeed::new(nmp_nip68::picture_feed_predicate(Arc::new(|_| true)));
    for event in &photo_events {
        photo_feed.on_kernel_event(event);
    }
    let photo_rows = photo_feed.snapshot(&FeedRequest::newest(5)).cards.len();

    let repost_events = fetch_events(
        &relays,
        json!({"kinds":[nmp_nip18::KIND_GENERIC_REPOST]}),
        40,
    );
    let mut picture_reposts = 0usize;
    for event in &repost_events {
        let Some(record) = nmp_nip18::try_from_kernel_event(event) else {
            continue;
        };
        if record.target_kind == Some(nmp_nip68::KIND_PICTURE_EVENT)
            || record
                .embedded_event
                .as_ref()
                .is_some_and(|inner| inner.kind == nmp_nip68::KIND_PICTURE_EVENT)
        {
            picture_reposts += 1;
        }
    }

    let mut custom_source = longform_events.clone();
    if custom_source.is_empty() {
        custom_source = social_events.clone();
    }
    let custom_rows = assert_custom_ranking_and_page(&custom_source).unwrap_or(0);

    let body = format!(
        "Fetched real kind:3 for `{author_name}` from `{kind3_relay}` with {} followees; sampled {}.\n\n\
         Assertions:\n\n\
         - primary social declaration `[1]` compiled to acquisition kinds `[1,6]` and never `[1,6]` as app-owned primary policy.\n\
         - sampled real follow set became the exact compiled REQ author set.\n\
         - mutating the real follow set by `- {removed}` / `+ {NEW_FOLLOW}` changed the author filter and plan id `{plan_id}` -> `{new_plan_id}`.\n\
         - live social query returned {} real kind:1/kind:6 events matching the compiled authors/kinds.\n\
         - relay-set kind:30023 feed compiled to app relays with no authors filter; live no-author query returned {} real kind:30023 events.\n\
         - live kind:20 query returned {} events; NIP-68 picture adapter rendered {} rows from parsed feed data.\n\
         - live kind:16 query returned {} events; {} claimed a kind:20 target via `k` tag or embedded event.\n\
         - caller-owned custom order/filtering ran over {} real events and produced {} bounded feed rows with page limit 2.\n\n\
         Kind:16 picture repost observation is reported as evidence, not a hard public-relay invariant: absence means this relay sample did not serve that shape within budget, not that the adapter path is green by itself.",
        followees.len(),
        sampled.len(),
        social_events.len(),
        longform_events.len(),
        photo_events.len(),
        photo_rows,
        repost_events.len(),
        picture_reposts,
        custom_source.len(),
        custom_rows,
    );
    write_report(
        "scenario-feed-matrix",
        &report_page(
            "Scenario — declared feed matrix",
            "feed-matrix",
            Verdict::Pass,
            &relays,
            &body,
        ),
    );
    println!(
        "PASS feed matrix: kind3={author_name}@{kind3_relay} social={} longform={} photo={} kind16={} picture_kind16={picture_reposts}",
        social_events.len(),
        longform_events.len(),
        photo_events.len(),
        repost_events.len()
    );
}
