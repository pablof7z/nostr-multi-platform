//! Hermetic topic-article feed proof for issue #1626.
//!
//! The app-level topic-article action opens two relay-expressible lanes:
//! direct `kind:30023 #t=<topic>` and generic repost `kind:16 #k=30023`.
//! This fixture sends both through real local REQ/EVENT/EOSE WebSocket
//! boundaries, then renders with the app-neutral long-form feed adapter. The
//! feed admits direct articles plus topic-proven kind:16 wrappers and does not
//! fetch kind:0 profiles or unresolved targets as feed-owned dependencies.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::FeedRequest;
use nmp_planner::LogicalInterest;
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Map, Value};
use tungstenite::Message;

const TOPIC: &str = "nostr";
const OTHER_TOPIC: &str = "music";

fn longform_event(keys: &Keys, d_tag: &str, topic: &str, created_at: u64, content: &str) -> Event {
    EventBuilder::new(
        Kind::from_u16(nmp_content::KIND_LONG_FORM_ARTICLE as u16),
        content,
    )
    .tags([
        Tag::parse(["d", d_tag]).unwrap(),
        Tag::parse(["title", &format!("title {d_tag}")]).unwrap(),
        Tag::parse(["summary", &format!("summary {d_tag}")]).unwrap(),
        Tag::parse(["image", &format!("https://img.example/{d_tag}.jpg")]).unwrap(),
        Tag::parse(["t", topic]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign kind:30023 article")
}

fn generic_repost(keys: &Keys, target: &Event, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::from_u16(nmp_nip18::KIND_GENERIC_REPOST as u16),
        target.as_json(),
    )
    .tags([
        Tag::parse(["e", &target.id.to_hex()]).unwrap(),
        Tag::parse(["p", &target.pubkey.to_hex()]).unwrap(),
        Tag::parse(["k", &nmp_content::KIND_LONG_FORM_ARTICLE.to_string()]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign kind:16 repost")
}

fn tag_only_repost(keys: &Keys, target_id: &str, created_at: u64) -> Event {
    EventBuilder::new(Kind::from_u16(nmp_nip18::KIND_GENERIC_REPOST as u16), "")
        .tags([
            Tag::parse(["e", target_id]).unwrap(),
            Tag::parse(["k", &nmp_content::KIND_LONG_FORM_ARTICLE.to_string()]).unwrap(),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign unresolved kind:16 repost")
}

fn profile_event(keys: &Keys) -> Event {
    EventBuilder::new(Kind::from_u16(0), r#"{"name":"not-feed-owned"}"#)
        .custom_created_at(Timestamp::from_secs(500))
        .sign_with_keys(keys)
        .expect("sign kind:0 profile")
}

fn spawn_one_req_relay(events: Vec<Event>) -> (String, thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture relay");
    let addr = listener.local_addr().expect("fixture relay addr");
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture relay accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("fixture relay read timeout");
        let mut ws = tungstenite::accept(stream).expect("fixture relay websocket accept");
        let req = loop {
            match ws.read().expect("fixture relay read") {
                Message::Text(text) if text.starts_with("[\"REQ\"") => break text,
                _ => {}
            }
        };
        let parsed: Value = serde_json::from_str(&req).expect("valid REQ JSON");
        let arr = parsed.as_array().expect("REQ array");
        let sub_id = arr.get(1).and_then(Value::as_str).unwrap_or("sub");
        let filter = arr.get(2).cloned().unwrap_or(Value::Null);
        for event in events.iter().filter(|event| matches_filter(event, &filter)) {
            ws.send(Message::Text(json!(["EVENT", sub_id, event]).to_string()))
                .expect("send EVENT");
        }
        ws.send(Message::Text(json!(["EOSE", sub_id]).to_string()))
            .expect("send EOSE");
        let _ = ws.close(None);
        filter
    });
    (format!("ws://{addr}"), handle)
}

fn matches_filter(event: &Event, filter: &Value) -> bool {
    let kinds = integer_set(filter.get("kinds"));
    let topic_tags = string_set(filter.get("#t"));
    let kind_tags = string_set(filter.get("#k"));
    (kinds.is_empty() || kinds.contains(&u64::from(event.kind.as_u16())))
        && (topic_tags.is_empty() || event_has_tag(event, "t", &topic_tags))
        && (kind_tags.is_empty() || event_has_tag(event, "k", &kind_tags))
}

fn event_has_tag(event: &Event, name: &str, allowed: &BTreeSet<String>) -> bool {
    event.tags.iter().any(|tag| {
        let vec = tag.clone().to_vec();
        vec.first().is_some_and(|tag_name| tag_name == name)
            && vec.get(1).is_some_and(|value| allowed.contains(value))
    })
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn integer_set(value: Option<&Value>) -> BTreeSet<u64> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect()
}

fn filter_from_interest(interest: &LogicalInterest) -> Value {
    let mut filter = Map::new();
    filter.insert(
        "kinds".to_string(),
        Value::Array(
            interest
                .shape
                .kinds
                .iter()
                .copied()
                .map(|kind| Value::from(u64::from(kind)))
                .collect(),
        ),
    );
    for (tag, values) in &interest.shape.tags {
        filter.insert(
            format!("#{tag}"),
            Value::Array(values.iter().cloned().map(Value::from).collect()),
        );
    }
    if let Some(limit) = interest.shape.limit {
        filter.insert("limit".to_string(), Value::from(u64::from(limit)));
    }
    Value::Object(filter)
}

fn fetch_kernel_events(url: &str, filter: Value, sub_id: &str) -> Vec<KernelEvent> {
    let (mut ws, _) = tungstenite::connect(url).expect("connect fixture relay");
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("set client timeout");
    }
    ws.send(Message::Text(json!(["REQ", sub_id, filter]).to_string()))
        .expect("send REQ");

    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Text(text)) if is_eose(&text, sub_id) => break,
            Ok(Message::Text(text)) => {
                if let Some(event) = parse_event_frame(&text, sub_id, url) {
                    out.push(event);
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("fixture relay read failed: {e}"),
        }
    }
    let _ = ws.close(None);
    out
}

fn parse_event_frame(text: &str, sub_id: &str, relay: &str) -> Option<KernelEvent> {
    let value: Value = serde_json::from_str(text).ok()?;
    let arr = value.as_array()?;
    if arr.first()?.as_str()? != "EVENT" || arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let event = arr.get(2)?.as_object()?;
    let tags = event
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
        id: event.get("id")?.as_str()?.to_string(),
        author: event.get("pubkey")?.as_str()?.to_string(),
        kind: event.get("kind")?.as_u64()? as u32,
        created_at: event.get("created_at")?.as_u64()?,
        tags,
        content: event
            .get("content")?
            .as_str()
            .unwrap_or_default()
            .to_string(),
        relay_provenance: vec![relay.to_string()],
    })
}

fn is_eose(text: &str, sub_id: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| {
            let arr = value.as_array()?;
            Some(
                arr.first().and_then(Value::as_str) == Some("EOSE")
                    && arr.get(1).and_then(Value::as_str) == Some(sub_id),
            )
        })
        .unwrap_or(false)
}

fn address(author: &Keys, d_tag: &str) -> String {
    format!(
        "{}:{}:{d_tag}",
        nmp_content::KIND_LONG_FORM_ARTICLE,
        author.public_key().to_hex()
    )
}

#[test]
fn topic_article_feed_renders_kind30023_and_kind16_without_secondary_hydration() {
    let direct_author = Keys::generate();
    let target_author = Keys::generate();
    let reposter = Keys::generate();

    let direct = longform_event(&direct_author, "direct", TOPIC, 200, "direct topic article");
    let target = longform_event(&target_author, "target", TOPIC, 100, "target topic article");
    let non_topic = longform_event(&target_author, "other", OTHER_TOPIC, 150, "other article");
    let topic_repost = generic_repost(&reposter, &target, 300);
    let non_topic_repost = generic_repost(&reposter, &non_topic, 350);
    let unresolved_repost = tag_only_repost(&reposter, &"f".repeat(64), 400);
    let profile = profile_event(&direct_author);
    let events = vec![
        direct.clone(),
        target.clone(),
        non_topic.clone(),
        topic_repost.clone(),
        non_topic_repost.clone(),
        unresolved_repost.clone(),
        profile.clone(),
    ];

    let direct_interest = nmp_defaults::topic_articles::topic_articles_interest(TOPIC);
    let repost_interest = nmp_defaults::topic_articles::topic_article_reposts_interest(TOPIC);
    let direct_filter = filter_from_interest(&direct_interest);
    let repost_filter = filter_from_interest(&repost_interest);

    let (direct_relay, direct_thread) = spawn_one_req_relay(events.clone());
    let direct_events = fetch_kernel_events(&direct_relay, direct_filter, "topic-direct");
    let observed_direct_filter = direct_thread.join().expect("direct relay thread");

    let (repost_relay, repost_thread) = spawn_one_req_relay(events);
    let repost_events = fetch_kernel_events(&repost_relay, repost_filter, "topic-reposts");
    let observed_repost_filter = repost_thread.join().expect("repost relay thread");

    assert_eq!(
        integer_set(observed_direct_filter.get("kinds")),
        [nmp_content::KIND_LONG_FORM_ARTICLE as u64]
            .into_iter()
            .collect()
    );
    assert_eq!(
        string_set(observed_direct_filter.get("#t")),
        [TOPIC.to_string()].into_iter().collect()
    );
    assert!(
        !observed_direct_filter
            .as_object()
            .unwrap()
            .contains_key("authors"),
        "topic article feed is not secretly an author/follow feed"
    );

    assert_eq!(
        integer_set(observed_repost_filter.get("kinds")),
        [nmp_nip18::KIND_GENERIC_REPOST as u64]
            .into_iter()
            .collect()
    );
    assert_eq!(
        string_set(observed_repost_filter.get("#k")),
        [nmp_content::KIND_LONG_FORM_ARTICLE.to_string()]
            .into_iter()
            .collect()
    );
    assert!(
        !observed_repost_filter
            .as_object()
            .unwrap()
            .contains_key("#t"),
        "kind:16 wrapper topic membership is adapter-owned, not relay-expressible"
    );

    assert_eq!(
        direct_events
            .iter()
            .map(|event| &event.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([&direct.id.to_hex(), &target.id.to_hex()]),
        "direct lane fetches only topic-tagged kind:30023 rows"
    );
    assert_eq!(
        repost_events
            .iter()
            .map(|event| &event.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &topic_repost.id.to_hex(),
            &non_topic_repost.id.to_hex(),
            &unresolved_repost.id.to_hex(),
        ]),
        "repost lane fetches generic long-form wrappers but no profiles or targets"
    );
    assert!(
        direct_events
            .iter()
            .chain(repost_events.iter())
            .all(|event| event.id != profile.id.to_hex()),
        "kind:0 profiles are not feed-owned dependencies"
    );

    let feed = nmp_content::LongformFeed::for_topic(
        TOPIC,
        nmp_content::longform_feed_predicate(Arc::new(|_| true)),
        Arc::new(|_| None),
    );
    for event in direct_events.iter().chain(repost_events.iter()) {
        feed.on_kernel_event(event);
    }

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(
        snapshot
            .cards
            .iter()
            .map(|row| row.card.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            address(&target_author, "target"),
            address(&direct_author, "direct"),
        ],
        "matching kind:16 wrapper positions the target article; non-topic and unresolved wrappers stay out"
    );
    let reposted = &snapshot.cards[0].card;
    assert_eq!(reposted.article.as_ref().unwrap().id, target.id.to_hex());
    assert_eq!(
        reposted.reposted_by.as_ref().unwrap().author_pubkey,
        reposter.public_key().to_hex()
    );
    assert_eq!(
        reposted.reposted_by.as_ref().unwrap().repost_created_at,
        300
    );
    assert_eq!(
        snapshot.cards[1].card.article.as_ref().unwrap().id,
        direct.id.to_hex()
    );
    assert!(snapshot.cards[1].card.reposted_by.is_none());
}
