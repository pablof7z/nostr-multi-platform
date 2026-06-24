//! Relay-set declared-feed fixture for issue #1626.
//!
//! Unit tests already prove the compiler emits no-author filters for relay-set
//! feeds. This test sends those exact compiled REQ filters through local
//! WebSocket relays so the boundary is exercised as relay traffic, not only as
//! in-memory shape data.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use nmp_core::subs::{SubscriptionLifecycle, WireFrame};
use nmp_planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};
use tungstenite::Message;

const KIND_LONGFORM: u32 = 30_023;

fn longform_event(keys: &Keys, created_at: u64, slug: &str, content: &str) -> Event {
    EventBuilder::new(Kind::from_u16(KIND_LONGFORM as u16), content)
        .tags([Tag::parse(["d", slug]).unwrap()])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:30023 longform")
}

fn relay_set_longform_feed() -> LogicalInterest {
    LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [KIND_LONGFORM].into_iter().collect(),
            ..Default::default()
        },
        lifecycle: InterestLifecycle::Tailing,
        ..Default::default()
    }
}

fn spawn_recording_relay(events: Vec<Event>) -> (String, thread::JoinHandle<Value>) {
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
    let authors = string_set(filter.get("authors"));
    (kinds.is_empty() || kinds.contains(&u64::from(event.kind.as_u16())))
        && (authors.is_empty() || authors.contains(&event.pubkey.to_hex()))
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

fn fetch_event_count(relay_url: &str, filter: &str) -> usize {
    let (mut ws, _) = tungstenite::connect(relay_url).expect("connect fixture relay");
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("set client timeout");
    }
    let sub_id = "relay-set-fixture";
    let filter: Value = serde_json::from_str(filter).expect("compiled filter JSON");
    ws.send(Message::Text(json!(["REQ", sub_id, filter]).to_string()))
        .expect("send REQ");

    let mut count = 0usize;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Text(text)) if is_eose(&text, sub_id) => break,
            Ok(Message::Text(text)) => {
                if is_event(&text, sub_id) {
                    count += 1;
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
    count
}

fn is_event(text: &str, sub_id: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| {
            let arr = value.as_array()?;
            Some(
                arr.first().and_then(Value::as_str) == Some("EVENT")
                    && arr.get(1).and_then(Value::as_str) == Some(sub_id),
            )
        })
        .unwrap_or(false)
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

#[test]
fn relay_set_kind30023_feed_reqs_have_no_author_or_tag_filters_at_relay_boundary() {
    let author_a = Keys::generate();
    let author_b = Keys::generate();
    let (relay_a, handle_a) = spawn_recording_relay(vec![longform_event(
        &author_a,
        100,
        "relay-a",
        "relay A longform",
    )]);
    let (relay_b, handle_b) = spawn_recording_relay(vec![longform_event(
        &author_b,
        200,
        "relay-b",
        "relay B longform",
    )]);

    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle.set_indexer_relays(vec!["wss://indexer.example".to_string()]);
    lifecycle.set_app_relays(vec![relay_a.clone(), relay_b.clone()]);
    let interest = relay_set_longform_feed();
    let token = nmp_core::kernel::cache_serve::RegistryWriteToken::for_test();
    let identity = nmp_core::subs::SubIdentity::for_standing_interest(&interest);
    let _ = lifecycle.registry_mut().apply(
        &token,
        nmp_core::kernel::cache_serve::InterestWrite::Replace,
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
                ..
            } => Some((relay_url.as_str(), filter_json.as_str())),
            WireFrame::Close { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reqs.iter()
            .map(|(relay, _)| *relay)
            .collect::<BTreeSet<_>>(),
        [relay_a.as_str(), relay_b.as_str()].into_iter().collect(),
        "relay-set feeds must route to the app relay set, not indexers"
    );

    let mut counts = Vec::new();
    for (relay_url, filter_json) in &reqs {
        counts.push(fetch_event_count(relay_url, filter_json));
    }
    assert_eq!(
        counts,
        vec![1, 1],
        "compiled filters must retrieve relay rows"
    );

    for observed in [handle_a.join().unwrap(), handle_b.join().unwrap()] {
        assert_eq!(observed.get("kinds"), Some(&json!([KIND_LONGFORM])));
        assert!(
            observed.get("authors").is_none(),
            "relay-set feed must not gain authors: {observed}"
        );
        assert!(
            observed.get("#p").is_none()
                && observed.get("#a").is_none()
                && observed.get("#e").is_none(),
            "relay-set feed must not gain tag filters: {observed}"
        );
    }
}
