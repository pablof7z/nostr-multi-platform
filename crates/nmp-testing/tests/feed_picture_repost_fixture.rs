//! Deterministic fixture-relay proof for issue #1626's kind:20/kind:16 feed
//! shape.
//!
//! The public real-relay matrix reports whether live relays happen to serve a
//! kind:16 repost of a kind:20 picture event. This test is the hermetic gate:
//! a local WebSocket relay serves signed NIP-01 events through a real REQ →
//! EVENT → EOSE boundary, then the app-neutral NIP-68 feed adapter renders the
//! result from a source-author perspective.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::FeedRequest;
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};
use tungstenite::Message;

const TARGET_CREATED_AT: u64 = 100;
const DIRECT_CREATED_AT: u64 = 150;
const REPOST_CREATED_AT: u64 = 200;

fn picture_event(keys: &Keys, created_at: u64, content: &str) -> Event {
    let draft = nmp_nip68::PicturePost::new(
        nmp_nip68::ImageMeta::new("https://cdn.example/picture.jpg")
            .mime("image/jpeg")
            .dimensions(1024, 768),
    )
    .title("fixture")
    .content(content)
    .build()
    .expect("valid picture draft");
    EventBuilder::new(
        Kind::from_u16(nmp_nip68::KIND_PICTURE_EVENT as u16),
        draft.content,
    )
    .tags(draft.tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign kind:20 picture")
}

fn generic_repost(keys: &Keys, target: &Event, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::from_u16(nmp_nip18::KIND_GENERIC_REPOST as u16),
        target.as_json(),
    )
    .tags([
        Tag::parse(["e", &target.id.to_hex()]).unwrap(),
        Tag::parse(["p", &target.pubkey.to_hex()]).unwrap(),
        Tag::parse(["k", &nmp_nip68::KIND_PICTURE_EVENT.to_string()]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign kind:16 repost")
}

fn spawn_one_req_relay(events: Vec<Event>) -> (String, thread::JoinHandle<()>) {
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
            let frame = json!(["EVENT", sub_id, event]).to_string();
            ws.send(Message::Text(frame)).expect("send EVENT");
        }
        ws.send(Message::Text(json!(["EOSE", sub_id]).to_string()))
            .expect("send EOSE");
        let _ = ws.close(None);
    });
    (format!("ws://{addr}"), handle)
}

fn matches_filter(event: &Event, filter: &Value) -> bool {
    let authors = string_set(filter.get("authors"));
    let kinds = integer_set(filter.get("kinds"));
    (authors.is_empty() || authors.contains(&event.pubkey.to_hex()))
        && (kinds.is_empty() || kinds.contains(&u64::from(event.kind.as_u16())))
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

fn fetch_kernel_events(url: &str, filter: Value) -> Vec<KernelEvent> {
    let (mut ws, _) = tungstenite::connect(url).expect("connect fixture relay");
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("set client timeout");
    }
    let sub_id = "kind16-fixture";
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

#[test]
fn fixture_relay_kind20_feed_gets_kind16_reposts_for_free() {
    let target_author = Keys::generate();
    let source_author = Keys::generate();
    let target = picture_event(&target_author, TARGET_CREATED_AT, "outside source");
    let direct = picture_event(&source_author, DIRECT_CREATED_AT, "source direct");
    let repost = generic_repost(&source_author, &target, REPOST_CREATED_AT);
    let source_pubkey = source_author.public_key().to_hex();

    let (relay_url, relay_thread) =
        spawn_one_req_relay(vec![target.clone(), direct.clone(), repost.clone()]);
    let acquisition_kinds = nmp_nip68::picture_acquisition_kinds();
    assert_eq!(
        acquisition_kinds,
        [
            nmp_nip68::KIND_PICTURE_EVENT,
            nmp_nip18::KIND_GENERIC_REPOST,
            nmp_nip18::KIND_DELETE,
        ]
        .into_iter()
        .collect()
    );

    let events = fetch_kernel_events(
        &relay_url,
        json!({
            "authors": [source_pubkey],
            "kinds": acquisition_kinds.iter().copied().collect::<Vec<_>>()
        }),
    );
    relay_thread.join().expect("fixture relay thread");

    assert_eq!(
        events.len(),
        2,
        "only source-authored direct + repost match"
    );
    assert!(events.iter().any(|event| event.id == direct.id.to_hex()));
    assert!(events.iter().any(|event| event.id == repost.id.to_hex()));
    assert!(
        events.iter().all(|event| event.id != target.id.to_hex()),
        "target author is outside the source perspective; target appears only via kind:16"
    );

    let feed =
        nmp_nip68::PictureFeed::new(nmp_nip68::picture_feed_predicate(Arc::new(move |author| {
            author == source_pubkey
        })));
    for event in &events {
        feed.on_kernel_event(event);
    }

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snapshot.cards.len(), 2);
    let reposted = &snapshot.cards[0].card;
    assert_eq!(reposted.id, target.id.to_hex());
    assert_eq!(
        reposted.record.as_ref().unwrap().author,
        target_author.public_key().to_hex()
    );
    assert_eq!(
        reposted.record.as_ref().unwrap().created_at,
        TARGET_CREATED_AT
    );
    assert_eq!(
        reposted.reposted_by.as_ref().unwrap().author_pubkey,
        source_author.public_key().to_hex()
    );
    assert_eq!(
        reposted.reposted_by.as_ref().unwrap().repost_created_at,
        REPOST_CREATED_AT
    );
    assert_eq!(snapshot.cards[1].card.id, direct.id.to_hex());
}
