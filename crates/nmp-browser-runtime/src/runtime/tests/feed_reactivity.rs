//! Browser feed-session source-graph reactivity gates.

use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::RelayFrame;
use nostr::JsonUtil;

const RELAY: &str = "wss://relay.example";
const BROWSER_FEED_KEY: &str = "test.browser.feed.reactivity";

#[test]
fn active_follow_feed_retargets_on_account_switch_without_app_intervention() {
    let viewer_one_keys = nostr::Keys::generate();
    let viewer_two_keys = nostr::Keys::generate();
    let follow_one_keys = nostr::Keys::generate();
    let follow_two_keys = nostr::Keys::generate();
    let viewer_one_pk = viewer_one_keys.public_key().to_hex();
    let viewer_two_pk = viewer_two_keys.public_key().to_hex();
    let follow_one_pk = follow_one_keys.public_key().to_hex();
    let follow_two_pk = follow_two_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    connect_content_relay(&mut handle);
    open_test_feed(&mut handle);

    let outbound = handle.apply_set_active_account(viewer_one_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_viewer_one = handle.pump();
    let contact_sub_one = req_sub_for_kind_and_author(
        &after_viewer_one.outbound,
        nmp_kinds::KIND_CONTACT_LIST,
        &viewer_one_pk,
    )
    .expect("viewer one must open its contact-list subscription");

    let contact_frame_one = relay_event_frame(
        &contact_sub_one,
        signed_kind3_json(&viewer_one_keys, std::slice::from_ref(&follow_one_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame_one),
    );
    handle.fan_out_outbound(outbound);
    let after_contact_one = handle.pump();
    let feed_sub_one = req_sub_for_kind_and_author(
        &after_contact_one.outbound,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        &follow_one_pk,
    )
    .expect("viewer one's follow list must open follow one's note subscription");

    let note_one_json = signed_note_json(&follow_one_keys, "viewer one follow", 20);
    let note_one_id = event_id_from_json(&note_one_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub_one, note_one_json)),
    );
    handle.fan_out_outbound(outbound);

    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_one_id),
        "viewer one's followed note must render before the switch"
    );

    let outbound = handle.apply_set_active_account(viewer_two_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_switch = handle.pump();
    let contact_sub_two = req_sub_for_kind_and_author(
        &after_switch.outbound,
        nmp_kinds::KIND_CONTACT_LIST,
        &viewer_two_pk,
    )
    .expect("switching active accounts must retarget the contact-list observer");

    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_one_id),
        "account switch must reset the rendered feed without app-side cleanup"
    );

    let contact_frame_two = relay_event_frame(
        &contact_sub_two,
        signed_kind3_json(&viewer_two_keys, std::slice::from_ref(&follow_two_pk), 30),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame_two),
    );
    handle.fan_out_outbound(outbound);
    let after_contact_two = handle.pump();
    let feed_sub_two = req_sub_for_kind_and_author(
        &after_contact_two.outbound,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        &follow_two_pk,
    )
    .expect("viewer two's follow list must open follow two's note subscription");

    let note_two_json = signed_note_json(&follow_two_keys, "viewer two follow", 40);
    let note_two_id = event_id_from_json(&note_two_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub_two, note_two_json)),
    );
    handle.fan_out_outbound(outbound);

    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_two_id),
        "viewer two's followed note must render after the switch"
    );
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_one_id),
        "viewer one's rendered rows must not survive the active-account switch"
    );
}

#[test]
fn active_follow_feed_replaces_follow_list_without_stale_rows() {
    let viewer_keys = nostr::Keys::generate();
    let follow_a_keys = nostr::Keys::generate();
    let follow_b_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_a_pk = follow_a_keys.public_key().to_hex();
    let follow_b_pk = follow_b_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    connect_content_relay(&mut handle);
    open_test_feed(&mut handle);

    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    let after_sign_in = handle.pump();
    let contact_sub = req_sub_for_kind_and_author(
        &after_sign_in.outbound,
        nmp_kinds::KIND_CONTACT_LIST,
        &viewer_pk,
    )
    .expect("active account must open contact-list subscription");

    let contact_frame_a = relay_event_frame(
        &contact_sub,
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_a_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame_a),
    );
    handle.fan_out_outbound(outbound);
    let after_contact_a = handle.pump();
    let feed_sub_a = req_sub_for_kind_and_author(
        &after_contact_a.outbound,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        &follow_a_pk,
    )
    .expect("first follow list must open follow A's note subscription");

    let note_a_json = signed_note_json(&follow_a_keys, "follow A", 20);
    let note_a_id = event_id_from_json(&note_a_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub_a, note_a_json)),
    );
    handle.fan_out_outbound(outbound);
    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_a_id),
        "follow A's note must render before the follow list changes"
    );

    let contact_frame_b = relay_event_frame(
        &contact_sub,
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_b_pk), 30),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame_b),
    );
    handle.fan_out_outbound(outbound);
    let after_contact_b = handle.pump();
    let feed_sub_b = req_sub_for_kind_and_author(
        &after_contact_b.outbound,
        nmp_kinds::KIND_SHORT_TEXT_NOTE,
        &follow_b_pk,
    )
    .expect("replacement follow list must open follow B's note subscription");
    assert!(
        req_sub_for_kind_and_author(
            &after_contact_b.outbound,
            nmp_kinds::KIND_SHORT_TEXT_NOTE,
            &follow_a_pk
        )
        .is_none(),
        "replacement demand must not reopen the withdrawn follow A author"
    );

    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "follow-list replacement must reset rows that belonged to the old source"
    );

    let note_b_json = signed_note_json(&follow_b_keys, "follow B", 40);
    let note_b_id = event_id_from_json(&note_b_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub_b, note_b_json)),
    );
    handle.fan_out_outbound(outbound);

    let feed = decode_feed(&handle.next_frame(true));
    assert!(
        feed.cards.iter().any(|card| card.card.id == note_b_id),
        "newly followed author's note must render after replacement"
    );
    assert!(
        feed.cards.iter().all(|card| card.card.id != note_a_id),
        "old follow rows must stay absent after the replacement source renders"
    );
}

fn connect_content_relay(handle: &mut crate::BrowserRuntimeHandle) {
    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        RELAY,
        false,
    );
    handle.fan_out_outbound(connected);
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

fn event_id_from_json(event_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(event_json)
        .expect("signed event json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed event has id")
        .to_string()
}

fn open_test_feed(handle: &mut crate::BrowserRuntimeHandle) -> nmp_feed::FeedHandle {
    handle
        .feeds()
        .open_spec(
            nmp_feed::FeedKey::app(BROWSER_FEED_KEY).unwrap(),
            nmp_feed::feed::events()
                .primary_kinds([nmp_kinds::KIND_SHORT_TEXT_NOTE])
                .from(nmp_feed::source::active_user().follows())
                .shape(nmp_feed::FeedShape::RootIndexed)
                .order(nmp_feed::FeedOrder::NewestByFeedPosition)
                .window(nmp_feed::FeedWindowPolicy::bounded(
                    nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
                ))
                .project(nmp_feed::FeedItemProjection::feed_rows()),
        )
        .expect("test-owned browser feed session opens")
}

fn req_sub_for_kind_and_author(
    outbound: &[nmp_core::OutboundMessage],
    kind: u32,
    author: &str,
) -> Option<String> {
    req_frames_for_kind(outbound, kind)
        .into_iter()
        .find(|(_, text)| req_frame_mentions_author(text, author))
        .map(|(sub_id, _)| sub_id)
}

fn req_frames_for_kind(outbound: &[nmp_core::OutboundMessage], kind: u32) -> Vec<(String, String)> {
    outbound
        .iter()
        .filter_map(|message| {
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
        .collect()
}

fn req_frame_mentions_author(text: &str, author: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let Some(arr) = value.as_array() else {
        return false;
    };
    arr.iter().skip(2).any(|filter| {
        filter
            .get("authors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|authors| {
                authors
                    .iter()
                    .any(|candidate| candidate.as_str() == Some(author))
            })
    })
}

fn decode_feed(frame: &crate::runtime::SnapshotOutcome) -> nmp_note_feed::op_feed::OpFeedSnapshot {
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
