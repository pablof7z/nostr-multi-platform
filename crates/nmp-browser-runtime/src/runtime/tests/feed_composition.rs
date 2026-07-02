//! Browser app-feed composition gates.

use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::{substrate::KernelEvent, RelayFrame};
use nostr::JsonUtil;

const ACCOUNT_PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_A_PK: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FOLLOW_NOTE_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RELAY: &str = "wss://relay.example";
const BROWSER_FEED_KEY: &str = "test.browser.feed.home";

#[test]
fn browser_startup_does_not_open_framework_owned_feed() {
    let handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert_eq!(
        handle.feed_sessions.live_count(),
        0,
        "browser startup must not open a framework-owned default feed session"
    );
}

#[test]
fn browser_home_feed_has_no_production_register_path() {
    let builder_source = include_str!("../../builder.rs");
    assert!(
        !builder_source.contains("register_browser_home_feed"),
        "BrowserAppBuilder::start must not install browser home feed through the \
         production register_browser_home_feed path"
    );
}

#[test]
fn browser_home_feed_close_tears_down_projection_and_provider() {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    let feed_handle = open_test_feed(&mut handle);
    assert_eq!(handle.feed_sessions.live_count(), 1);
    assert!(
        handle
            .runtime
            .reducer
            .registered_feed_author_provider_keys()
            .iter()
            .any(|key| key == BROWSER_FEED_KEY),
        "opening the caller-owned feed session must pair the typed projection with a \
         feed-author provider"
    );

    assert!(handle.close_feed(&feed_handle));
    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(!handle.close_feed(&feed_handle));
    assert!(
        !handle
            .runtime
            .reducer
            .registered_feed_author_provider_keys()
            .iter()
            .any(|key| key == BROWSER_FEED_KEY),
        "closing the session must remove the paired feed-author provider"
    );

    let bytes = handle.make_update_frame(true);
    let rows = nmp_core::decode_snapshot_typed_projections(&bytes).expect("frame decodes");
    let row = rows
        .iter()
        .find(|row| row.key == BROWSER_FEED_KEY)
        .expect("closed caller-owned feed projection emits a Cleared row");
    assert_eq!(row.state, nmp_core::WireProjectionState::Cleared);
}

#[test]
fn browser_home_feed_observer_opens_on_active_account_change() {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    open_test_feed(&mut handle);
    for role in [
        nmp_network::role::RelayRole::Content,
        nmp_network::role::RelayRole::Indexer,
    ] {
        let connected = handle
            .runtime
            .reducer
            .handle_relay_connected(role, RELAY, false);
        handle.fan_out_outbound(connected);
    }

    let outbound = handle.apply_set_active_account(ACCOUNT_PK.to_string());
    handle.fan_out_outbound(outbound);
    let out = handle.pump();

    let texts = out
        .outbound
        .iter()
        .map(|frame| frame.text().to_string())
        .collect::<Vec<_>>();
    assert!(
        texts
            .iter()
            .any(|text| text.contains(r#""kinds":[3]"#) && text.contains(ACCOUNT_PK)),
        "active-account change must open the contact-list observer; outbound={texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains(r#""kinds":[1,5,6]"#) && text.contains(ACCOUNT_PK)),
        "active-account change must open the self-included app-feed observer; outbound={texts:?}"
    );
}

#[test]
fn browser_home_feed_fails_closed_before_sign_in() {
    let note_keys = nostr::Keys::generate();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        RELAY,
        false,
    );
    handle.fan_out_outbound(connected);

    open_test_feed(&mut handle);
    let first_pump = handle.pump();
    assert!(
        req_frame_for_kind(&first_pump.outbound, nmp_kinds::KIND_SHORT_TEXT_NOTE).is_none(),
        "signed-out ActiveUserFollows must not open a public note subscription; outbound={:?}",
        first_pump
            .outbound
            .iter()
            .map(|frame| frame.text())
            .collect::<Vec<_>>()
    );

    let note_json = signed_note_json(&note_keys, "signed-out active follows should ignore", 20);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame("unsigned-active-follows", note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(
        feed.cards.len(),
        0,
        "signed-out ActiveUserFollows must fail closed instead of rendering public notes"
    );
}

#[test]
fn browser_home_feed_projection_renders_followed_note() {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    open_test_feed(&mut handle);
    let outbound = handle.apply_set_active_account(ACCOUNT_PK.to_string());
    handle.fan_out_outbound(outbound);
    handle.pump();

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&contact_list_event());
    handle.pump();

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&follow_note_event());

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(feed.cards.len(), 1, "followed kind:1 note must render");
    assert_eq!(feed.cards[0].card.id, FOLLOW_NOTE_ID);
    assert_eq!(feed.cards[0].card.author_pubkey, FOLLOW_A_PK);
    assert_eq!(feed.cards[0].card.content, "hello from runtime composition");
}

#[test]
fn browser_home_feed_reads_follow_set_from_stored_kind3() {
    let viewer_keys = nostr::Keys::generate();
    let follow_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = follow_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    open_test_feed(&mut handle);
    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    handle.pump();

    let contact_frame = relay_event_frame(
        "contact-list-sub",
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame),
    );
    handle.fan_out_outbound(outbound);

    let follows = latest_kind3_reader(&handle)
        .follows(&viewer_pk)
        .expect("stored kind:3 exists for active account");
    assert_eq!(
        follows.iter().map(String::as_str).collect::<Vec<_>>(),
        vec![follow_pk.as_str()],
        "browser feed follow admission must be backed by the event-store kind:3"
    );
}

#[test]
fn browser_home_feed_projection_renders_followed_note_from_relay_frames() {
    let viewer_keys = nostr::Keys::generate();
    let follow_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = follow_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    open_test_feed(&mut handle);
    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    handle.pump();

    let contact_frame = relay_event_frame(
        "contact-list-sub",
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame),
    );
    handle.fan_out_outbound(outbound);
    handle.pump();

    let note_json = signed_note_json(&follow_keys, "hello from relay frame", 20);
    let note_id = serde_json::from_str::<serde_json::Value>(&note_json)
        .expect("signed note json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed note has id")
        .to_string();
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame("follow-feed-sub", note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(feed.cards.len(), 1);
    assert_eq!(feed.cards[0].card.id, note_id);
    assert_eq!(feed.cards[0].card.author_pubkey, follow_pk);
    assert_eq!(feed.cards[0].card.content, "hello from relay frame");
}

#[test]
fn browser_home_feed_projection_renders_followed_note_from_runtime_wire_subs() {
    let viewer_keys = nostr::Keys::generate();
    let follow_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = follow_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        RELAY,
        false,
    );
    handle.fan_out_outbound(connected);

    open_test_feed(&mut handle);
    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    let first_pump = handle.pump();
    let contact_sub = req_sub_for_kind(&first_pump.outbound, nmp_kinds::KIND_CONTACT_LIST)
        .expect("active account must open contact-list subscription");

    let contact_frame = relay_event_frame(
        &contact_sub,
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame),
    );
    handle.fan_out_outbound(outbound);

    let after_contact = handle.pump();
    let feed_sub = req_sub_for_kind(&after_contact.outbound, nmp_kinds::KIND_SHORT_TEXT_NOTE)
        .expect("contact list must open followed-note subscription");

    let note_json = signed_note_json(&follow_keys, "hello through real wire sub", 20);
    let note_id = serde_json::from_str::<serde_json::Value>(&note_json)
        .expect("signed note json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed note has id")
        .to_string();
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub, note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(feed.cards.len(), 1);
    assert_eq!(feed.cards[0].card.id, note_id);
    assert_eq!(feed.cards[0].card.author_pubkey, follow_pk);
    assert_eq!(feed.cards[0].card.content, "hello through real wire sub");
}

fn contact_list_event() -> KernelEvent {
    KernelEvent {
        id: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
        author: ACCOUNT_PK.to_string(),
        kind: nmp_kinds::KIND_CONTACT_LIST,
        created_at: 10,
        tags: vec![vec!["p".to_string(), FOLLOW_A_PK.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn follow_note_event() -> KernelEvent {
    KernelEvent {
        id: FOLLOW_NOTE_ID.to_string(),
        author: FOLLOW_A_PK.to_string(),
        kind: nmp_kinds::KIND_SHORT_TEXT_NOTE,
        created_at: 20,
        tags: Vec::new(),
        content: "hello from runtime composition".to_string(),
        relay_provenance: vec![RELAY.to_string()],
    }
}

fn signed_kind3_json(keys: &nostr::Keys, follows: &[String], created_at: u64) -> String {
    let tags = follows
        .iter()
        .map(|pk| nostr::Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect::<Vec<_>>();
    nostr::EventBuilder::new(nostr::Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3")
        .as_json()
}

fn signed_note_json(keys: &nostr::Keys, content: &str, created_at: u64) -> String {
    nostr::EventBuilder::text_note(content)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
        .as_json()
}

fn relay_event_frame(sub_id: &str, event_json: String) -> String {
    format!(r#"["EVENT","{sub_id}",{event_json}]"#)
}

fn open_test_feed(handle: &mut crate::BrowserRuntimeHandle) -> nmp_feed::FeedHandle {
    let params = nmp_feed::FeedParams {
        primary_kinds: vec![nmp_kinds::KIND_SHORT_TEXT_NOTE],
        shape: nmp_feed::FeedShape::RootIndexed,
        acquisition: nmp_feed::FeedScope::ActiveUserFollows,
        admission: nmp_feed::FeedAdmission::All,
        ranking: nmp_feed::FeedRanking::ChronologicalDesc,
        window: nmp_feed::FeedWindow {
            initial_limit: nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        },
        projection: nmp_feed::ProjectionKey::app_owned(BROWSER_FEED_KEY).unwrap(),
    };
    handle
        .open_feed(params)
        .expect("test-owned browser feed session opens")
}

fn latest_kind3_reader(handle: &crate::BrowserRuntimeHandle) -> nmp_nip02::LatestKind3FollowSet {
    let slot = nmp_core::slots::new_event_store_slot();
    *slot.lock().expect("event-store slot") = Some(handle.event_store_handle());
    nmp_nip02::LatestKind3FollowSet::new(slot)
}

fn req_sub_for_kind(outbound: &[nmp_core::OutboundMessage], kind: u32) -> Option<String> {
    req_frame_for_kind(outbound, kind).map(|(sub_id, _)| sub_id)
}

fn req_frame_for_kind(
    outbound: &[nmp_core::OutboundMessage],
    kind: u32,
) -> Option<(String, String)> {
    outbound.iter().find_map(|message| {
        let value = serde_json::from_str::<serde_json::Value>(message.text()).ok()?;
        let arr = value.as_array()?;
        if arr.first()?.as_str()? != "REQ" {
            return None;
        }
        let sub_id = arr.get(1)?.as_str()?.to_string();
        let has_kind = arr.iter().skip(2).any(|filter| {
            filter
                .get("kinds")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|candidate| candidate.as_u64() == Some(kind as u64))
                })
        });
        has_kind.then_some((sub_id, message.text().to_string()))
    })
}

fn decode_home_feed(
    frame: &crate::runtime::SnapshotOutcome,
) -> nmp_note_feed::op_feed::OpFeedSnapshot {
    let crate::runtime::SnapshotOutcome::Frame(bytes) = frame else {
        panic!("expected snapshot frame, got {frame:?}");
    };
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).expect("frame decodes");
    let row = typed
        .into_iter()
        .find(|row| row.key == BROWSER_FEED_KEY)
        .expect("caller-owned feed projection must be present");
    nmp_note_feed::op_feed::decode_op_feed_snapshot(&row.payload).expect("NNFS payload decodes")
}
