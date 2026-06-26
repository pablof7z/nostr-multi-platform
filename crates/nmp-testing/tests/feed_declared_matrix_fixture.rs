//! Hermetic declared-feed matrix for issue #1626.
//!
//! Apps declare primary content kinds only. This fixture uses signed Nostr
//! events through a real local REQ/EVENT/EOSE WebSocket boundary to prove that
//! a mixed declaration `[1, 20]` derives both repost wrapper kinds without
//! turning target/profile acquisition into feed responsibility.

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nmp_core::substrate::{empty_suppression_lookup, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_feed::FeedRequest;
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};
use tungstenite::Message;

const TARGET_NOTE_TS: u64 = 100;
const SOURCE_NOTE_TS: u64 = 150;
const TARGET_PICTURE_TS: u64 = 200;
const SOURCE_PICTURE_TS: u64 = 250;
const NOTE_REPOST_TS: u64 = 300;
const PICTURE_REPOST_TS: u64 = 400;

fn text_note(keys: &Keys, created_at: u64, content: &str) -> Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:1 note")
}

fn picture_event(keys: &Keys, created_at: u64, content: &str) -> Event {
    let draft = nmp_nip68::PicturePost::new(
        nmp_nip68::ImageMeta::new("https://cdn.example/matrix.jpg")
            .mime("image/jpeg")
            .dimensions(640, 480),
    )
    .title("matrix")
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

fn note_repost(keys: &Keys, target: &Event, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::from_u16(nmp_nip18::KIND_REPOST as u16),
        target.as_json(),
    )
    .tags([
        Tag::parse(["e", &target.id.to_hex()]).unwrap(),
        Tag::parse(["p", &target.pubkey.to_hex()]).unwrap(),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign kind:6 repost")
}

fn picture_repost(keys: &Keys, target: &Event, created_at: u64) -> Event {
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

fn profile_event(keys: &Keys) -> Event {
    EventBuilder::new(Kind::from_u16(0), r#"{"name":"not-feed-owned"}"#)
        .custom_created_at(Timestamp::from_secs(SOURCE_NOTE_TS))
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
            let frame = json!(["EVENT", sub_id, event]).to_string();
            ws.send(Message::Text(frame)).expect("send EVENT");
        }
        ws.send(Message::Text(json!(["EOSE", sub_id]).to_string()))
            .expect("send EOSE");
        let _ = ws.close(None);
        filter
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
    let sub_id = "declared-feed-matrix";
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
fn mixed_primary_kinds_derive_kind6_and_kind16_without_secondary_hydration() {
    let source_author = Keys::generate();
    let note_target_author = Keys::generate();
    let picture_target_author = Keys::generate();
    let source_pubkey = source_author.public_key().to_hex();

    let target_note = text_note(&note_target_author, TARGET_NOTE_TS, "outside note target");
    let direct_note = text_note(&source_author, SOURCE_NOTE_TS, "source note");
    let target_picture = picture_event(
        &picture_target_author,
        TARGET_PICTURE_TS,
        "outside picture target",
    );
    let direct_picture = picture_event(&source_author, SOURCE_PICTURE_TS, "source picture");
    let note_wrapper = note_repost(&source_author, &target_note, NOTE_REPOST_TS);
    let picture_wrapper = picture_repost(&source_author, &target_picture, PICTURE_REPOST_TS);
    let profile = profile_event(&source_author);

    let primary_kinds = [
        nmp_nip01::KIND_SHORT_TEXT_NOTE,
        nmp_nip68::KIND_PICTURE_EVENT,
    ];
    let acquisition_kinds =
        nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds).expect("primary kinds");
    assert_eq!(
        acquisition_kinds,
        BTreeSet::from([
            nmp_nip01::KIND_SHORT_TEXT_NOTE,
            nmp_nip18::KIND_REPOST,
            nmp_nip18::KIND_GENERIC_REPOST,
            nmp_nip68::KIND_PICTURE_EVENT,
            nmp_nip18::KIND_DELETE,
        ]),
        "mixed primary feed declaration derives both repost wrapper kinds and deletes"
    );
    assert!(
        nmp_nip18::try_acquisition_kinds_for_primary([1, nmp_nip18::KIND_REPOST]).is_err(),
        "apps must not redeclare kind:6 as primary"
    );
    assert!(
        nmp_nip18::try_acquisition_kinds_for_primary([20, nmp_nip18::KIND_GENERIC_REPOST]).is_err(),
        "apps must not redeclare kind:16 as primary"
    );

    let (relay_url, relay_thread) = spawn_one_req_relay(vec![
        target_note.clone(),
        direct_note.clone(),
        target_picture.clone(),
        direct_picture.clone(),
        note_wrapper.clone(),
        picture_wrapper.clone(),
        profile.clone(),
    ]);
    let events = fetch_kernel_events(
        &relay_url,
        json!({
            "authors": [source_pubkey],
            "kinds": acquisition_kinds.iter().copied().collect::<Vec<_>>()
        }),
    );
    let observed_filter = relay_thread.join().expect("fixture relay thread");

    assert_eq!(
        integer_set(observed_filter.get("kinds")),
        acquisition_kinds.iter().copied().map(u64::from).collect(),
        "wire REQ uses derived acquisition kinds, not app-declared wrapper primaries"
    );
    assert_eq!(
        events.len(),
        4,
        "profile and outside target authors are not fetched"
    );
    let event_ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        event_ids,
        BTreeSet::from([
            direct_note.id.to_hex(),
            direct_picture.id.to_hex(),
            note_wrapper.id.to_hex(),
            picture_wrapper.id.to_hex(),
        ])
    );
    assert!(
        !event_ids.contains(&target_note.id.to_hex())
            && !event_ids.contains(&target_picture.id.to_hex())
            && !event_ids.contains(&profile.id.to_hex()),
        "targets/profiles must not be acquired as separate feed rows"
    );

    let follow_predicate: nmp_feed::FollowPredicate = Arc::new({
        let source_pubkey = source_author.public_key().to_hex();
        move |author| author == source_pubkey
    });
    let op_feed =
        nmp_nip01::register_op_feed("viewer".to_string(), follow_predicate, Arc::new(|_| None));
    let op_observer = nmp_nip01::op_feed::op_feed_observer(
        Arc::clone(&op_feed),
        Arc::new(|_| None),
        empty_suppression_lookup(),
    );
    let picture_feed = nmp_nip68::PictureFeed::with_event_lookup(
        nmp_nip68::picture_feed_predicate(Arc::new({
            let source_pubkey = source_author.public_key().to_hex();
            move |author| author == source_pubkey
        })),
        Arc::new(|_| None),
        None,
    );

    for event in &events {
        op_observer.on_kernel_event(event);
        picture_feed.on_kernel_event(event);
    }

    let op_snapshot = op_feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(op_snapshot.cards.len(), 2);
    assert_eq!(op_snapshot.cards[0].card.id, target_note.id.to_hex());
    assert_eq!(
        op_snapshot.cards[0].card.author_pubkey,
        note_target_author.public_key().to_hex()
    );
    assert_eq!(op_snapshot.cards[0].card.created_at, NOTE_REPOST_TS);
    assert_eq!(
        op_snapshot.cards[0]
            .card
            .reposted_by
            .as_ref()
            .expect("kind:6 attribution")
            .author_pubkey,
        source_author.public_key().to_hex()
    );
    assert_eq!(op_snapshot.cards[1].card.id, direct_note.id.to_hex());

    let picture_snapshot = picture_feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(picture_snapshot.cards.len(), 2);
    assert_eq!(
        picture_snapshot.cards[0].card.id,
        target_picture.id.to_hex()
    );
    assert_eq!(
        picture_snapshot.cards[0]
            .card
            .record
            .as_ref()
            .expect("kind:16 embedded target record")
            .author,
        picture_target_author.public_key().to_hex()
    );
    assert_eq!(
        picture_snapshot.cards[0]
            .card
            .reposted_by
            .as_ref()
            .expect("kind:16 attribution")
            .author_pubkey,
        source_author.public_key().to_hex()
    );
    assert_eq!(
        picture_snapshot.cards[1].card.id,
        direct_picture.id.to_hex()
    );
}
