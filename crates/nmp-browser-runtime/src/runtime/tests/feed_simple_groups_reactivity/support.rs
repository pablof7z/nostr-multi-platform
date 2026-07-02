use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::RelayFrame;
use nostr::JsonUtil;

use super::{GROUP_RELAY_A, GROUP_RELAY_B, LIST_RELAY};

pub(super) fn started_group_feed() -> crate::BrowserRuntimeHandle {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![
            (LIST_RELAY.to_string(), "both,indexer".to_string()),
            (GROUP_RELAY_A.to_string(), "both,indexer".to_string()),
            (GROUP_RELAY_B.to_string(), "both,indexer".to_string()),
        ])
        .decide_providers(BrowserRunConfig::default())
        .start();
    connect_content_relay(&mut handle, LIST_RELAY);
    connect_content_relay(&mut handle, GROUP_RELAY_A);
    connect_content_relay(&mut handle, GROUP_RELAY_B);
    handle
}

fn connect_content_relay(handle: &mut crate::BrowserRuntimeHandle, relay: &str) {
    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        relay,
        false,
    );
    handle.fan_out_outbound(connected);
}

pub(super) fn open_simple_group_feed(
    handle: &mut crate::BrowserRuntimeHandle,
    feed_key: &str,
) -> nmp_feed::FeedHandle {
    let params = nmp_feed::FeedParams {
        primary_kinds: vec![nmp_kinds::KIND_SHORT_TEXT_NOTE],
        shape: nmp_feed::FeedShape::RootIndexed,
        source: nmp_feed::FeedScope::ActiveUserHostedGroups,
        admission: nmp_feed::FeedAdmission::All,
        order: nmp_feed::FeedOrder::NewestByFeedPosition,
        window: nmp_feed::FeedWindowPolicy {
            initial_limit: nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        },
        projection: nmp_feed::ProjectionKey::app_owned(feed_key).unwrap(),
    };
    handle
        .open_feed(params)
        .expect("test-owned browser simple-groups feed session opens")
}

pub(super) fn signed_simple_groups_json(
    keys: &nostr::Keys,
    groups: &[(&str, &str)],
    created_at: u64,
) -> String {
    let tags = groups
        .iter()
        .map(|(local_id, relay)| {
            nostr::Tag::parse(["group", *local_id, *relay]).expect("valid group tag")
        })
        .collect::<Vec<_>>();
    nostr::EventBuilder::new(nostr::Kind::from(nmp_kinds::KIND_SIMPLE_GROUPS as u16), "")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10009")
        .as_json()
}

pub(super) fn signed_group_note_json(
    keys: &nostr::Keys,
    local_id: &str,
    content: &str,
    created_at: u64,
) -> String {
    nostr::EventBuilder::text_note(content)
        .tags([nostr::Tag::parse(["h", local_id]).expect("valid h tag")])
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign group note")
        .as_json()
}

pub(super) fn deliver(
    handle: &mut crate::BrowserRuntimeHandle,
    relay: &str,
    sub_id: &str,
    event_json: String,
) {
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        relay,
        RelayFrame::Text(relay_event_frame(sub_id, event_json)),
    );
    handle.fan_out_outbound(outbound);
}

fn relay_event_frame(sub_id: &str, event_json: String) -> String {
    format!(r#"["EVENT","{sub_id}",{event_json}]"#)
}

pub(super) fn event_id_from_json(event_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(event_json)
        .expect("signed event json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed event has id")
        .to_string()
}

pub(super) fn req_sub_for_kind_author_on(
    outbound: &[nmp_core::OutboundMessage],
    relay: &str,
    kind: u32,
    author: &str,
) -> Option<String> {
    req_frames_for_kind_on(outbound, relay, kind)
        .into_iter()
        .find(|(_, text)| req_frame_mentions_author(text, author))
        .map(|(sub_id, _)| sub_id)
}

pub(super) fn req_sub_for_kind_h_on(
    outbound: &[nmp_core::OutboundMessage],
    relay: &str,
    kind: u32,
    local_id: &str,
) -> Option<String> {
    req_frames_for_kind_on(outbound, relay, kind)
        .into_iter()
        .find(|(_, text)| req_frame_mentions_h(text, local_id))
        .map(|(sub_id, _)| sub_id)
}

fn req_frames_for_kind_on(
    outbound: &[nmp_core::OutboundMessage],
    relay: &str,
    kind: u32,
) -> Vec<(String, String)> {
    outbound
        .iter()
        .filter(|message| message.relay_url() == relay)
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

fn req_frame_mentions_h(text: &str, local_id: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let Some(arr) = value.as_array() else {
        return false;
    };
    arr.iter().skip(2).any(|filter| {
        filter
            .get("#h")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|candidate| candidate.as_str() == Some(local_id))
            })
    })
}

pub(super) fn decode_feed(
    frame: &crate::runtime::SnapshotOutcome,
    feed_key: &str,
) -> nmp_note_feed::op_feed::OpFeedSnapshot {
    let crate::runtime::SnapshotOutcome::Frame(bytes) = frame else {
        panic!("expected snapshot frame, got {frame:?}");
    };
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).expect("frame decodes");
    let row = typed
        .into_iter()
        .find(|row| row.key == feed_key)
        .expect("caller-owned feed projection must be present");
    nmp_note_feed::op_feed::decode_op_feed_snapshot(&row.payload).expect("NNFS payload decodes")
}
