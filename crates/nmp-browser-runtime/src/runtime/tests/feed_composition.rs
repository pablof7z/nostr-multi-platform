//! Browser home-feed composition gates.

use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::{substrate::KernelEvent, RelayFrame};
use nostr::JsonUtil;

const ACCOUNT_PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_A_PK: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FOLLOW_NOTE_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RELAY: &str = "wss://relay.example";

#[test]
fn browser_home_feed_startup_is_not_builder_observer_wiring() {
    let handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "browser home-feed startup must open one ordinary typed feed session"
    );
}

#[test]
fn browser_home_feed_has_no_production_register_path() {
    let builder_source = include_str!("../../builder.rs");
    assert!(
        !builder_source.contains("register_browser_home_feed"),
        "BrowserAppBuilder::start must not install browser home feed through the \
         production register_browser_home_feed path; startup should open the \
         typed home-feed session instead"
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

    assert_eq!(handle.feed_sessions.live_count(), 1);
    assert!(
        handle
            .runtime
            .reducer
            .registered_feed_author_provider_keys()
            .iter()
            .any(|key| key == nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY),
        "opening the home feed session must pair the typed projection with a \
         feed-author provider"
    );

    assert!(handle.close_home_feed_session());
    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(!handle.close_home_feed_session());
    assert!(
        !handle
            .runtime
            .reducer
            .registered_feed_author_provider_keys()
            .iter()
            .any(|key| key == nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY),
        "closing the session must remove the paired feed-author provider"
    );

    let bytes = handle.make_update_frame(true);
    let rows = nmp_core::decode_snapshot_typed_projections(&bytes).expect("frame decodes");
    let row = rows
        .iter()
        .find(|row| row.key == nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY)
        .expect("closed home feed projection emits a Cleared row");
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
        "active-account change must open the self-included home-feed observer; outbound={texts:?}"
    );
}

#[test]
fn browser_home_feed_projection_renders_public_note_before_sign_in() {
    let note_keys = nostr::Keys::generate();
    let note_pk = note_keys.public_key().to_hex();

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

    let first_pump = handle.pump();
    let (feed_sub, req_text) =
        req_frame_for_kind(&first_pump.outbound, nmp_kinds::KIND_SHORT_TEXT_NOTE)
            .expect("signed-out browser start must open a public note subscription");
    assert!(
        !req_text.contains(r#""authors""#),
        "signed-out public feed must not require a follow author filter; req={req_text}"
    );

    let note_json = signed_note_json(&note_keys, "hello from signed-out public feed", 20);
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
    assert_eq!(feed.cards[0].card.author_pubkey, note_pk);
    assert_eq!(
        feed.cards[0].card.content,
        "hello from signed-out public feed"
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
fn browser_home_feed_exports_follow_list_projection_from_contacts_lookup() {
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

    let frame = handle.next_frame(true);
    let follows = decode_follow_list(&frame);
    assert_eq!(
        follows
            .follows
            .iter()
            .map(|entry| entry.pubkey.as_str())
            .collect::<Vec<_>>(),
        vec![follow_pk.as_str()],
        "browser follow button state must be backed by the Rust NIP-02 projection"
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

fn decode_home_feed(frame: &crate::runtime::SnapshotOutcome) -> nmp_nip01::op_feed::OpFeedSnapshot {
    let crate::runtime::SnapshotOutcome::Frame(bytes) = frame else {
        panic!("expected snapshot frame, got {frame:?}");
    };
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).expect("frame decodes");
    let row = typed
        .into_iter()
        .find(|row| row.key == nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY)
        .expect("home feed projection must be present");
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload).expect("NOFS payload decodes")
}

fn decode_follow_list(frame: &crate::runtime::SnapshotOutcome) -> nmp_nip02::FollowListSnapshot {
    let crate::runtime::SnapshotOutcome::Frame(bytes) = frame else {
        panic!("expected snapshot frame, got {frame:?}");
    };
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).expect("frame decodes");
    let row = typed
        .into_iter()
        .find(|row| row.key == "nmp.follow_list")
        .expect("follow-list projection must be present");
    assert_eq!(row.schema_id, nmp_nip02::FOLLOW_LIST_SCHEMA_ID);
    nmp_nip02::decode_follow_list(&row.payload).expect("NF02 payload decodes")
}
