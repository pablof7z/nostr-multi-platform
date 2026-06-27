//! Browser home-feed reply-attribution gates.

use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::RelayFrame;
use nostr::JsonUtil;

const RELAY: &str = "wss://relay.example";

#[test]
fn browser_home_feed_projection_exports_reply_attribution() {
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
        signed_kind3_json(
            &viewer_keys,
            &[follow_a_pk.clone(), follow_b_pk.clone()],
            10,
        ),
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

    let root_json = signed_note_json(&follow_a_keys, "thread root from runtime composition", 20);
    let root_id = event_id_from_json(&root_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub, root_json)),
    );
    handle.fan_out_outbound(outbound);

    let reply_json = signed_reply_json(
        &follow_b_keys,
        "followed reply",
        30,
        &root_id,
        &[follow_a_pk.clone(), follow_b_pk.clone()],
    );
    let reply_id = event_id_from_json(&reply_json);
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub, reply_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    let card = feed
        .cards
        .iter()
        .find(|card| card.card.id == root_id)
        .expect("root card renders");
    assert_eq!(card.attribution.len(), 1);
    assert_eq!(card.attribution[0].author_pubkey, follow_b_pk);
    assert_eq!(card.attribution[0].reply_event_id, reply_id);
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

fn signed_reply_json(
    keys: &nostr::Keys,
    content: &str,
    created_at: u64,
    root_id: &str,
    participants: &[String],
) -> String {
    let mut tags = vec![nostr::Tag::parse(["e", root_id, "", "reply"]).expect("valid e tag")];
    tags.extend(
        participants
            .iter()
            .map(|pk| nostr::Tag::parse(["p", pk.as_str()]).expect("valid p tag")),
    );
    nostr::EventBuilder::new(nostr::Kind::TextNote, content)
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign reply")
        .as_json()
}

fn event_id_from_json(event_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(event_json)
        .expect("signed event json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed event has id")
        .to_string()
}

fn relay_event_frame(sub_id: &str, event_json: String) -> String {
    format!(r#"["EVENT","{sub_id}",{event_json}]"#)
}

fn req_sub_for_kind(outbound: &[nmp_core::OutboundMessage], kind: u32) -> Option<String> {
    outbound.iter().find_map(|message| {
        let value = serde_json::from_str::<serde_json::Value>(message.text()).ok()?;
        let arr = value.as_array()?;
        if arr.first()?.as_str()? != "REQ" {
            return None;
        }
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
        has_kind.then(|| arr.get(1)?.as_str().map(str::to_string))?
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
