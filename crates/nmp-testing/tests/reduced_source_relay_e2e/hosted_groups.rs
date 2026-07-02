use crate::common::recording_relay::{has_author, has_kind, RecordingRelay};
use crate::support::*;
use nmp_feed::{
    FeedAdmission, FeedItemProjection, FeedOrder, FeedParams, FeedScope, FeedShape,
    FeedWindowPolicy, ProjectionKey,
};
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

fn signed_simple_groups(keys: &Keys, groups: &[(&str, &str)], created_at: u64) -> Event {
    let tags: Vec<Tag> = groups
        .iter()
        .map(|(local_id, relay)| Tag::parse(["group", *local_id, *relay]).expect("valid group tag"))
        .collect();
    EventBuilder::new(Kind::from(10_009u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10009")
}

fn signed_group_message(keys: &Keys, local_id: &str, content: &str, created_at: u64) -> Event {
    EventBuilder::new(Kind::from(9u16), content)
        .tags([Tag::parse(["h", local_id]).expect("valid h tag")])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:9")
}

fn hosted_groups_params(key: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![9],
        shape: FeedShape::Flat,
        source: FeedScope::ActiveUserHostedGroups,
        admission: FeedAdmission::All,
        order: FeedOrder::NewestByFeedPosition,
        window: FeedWindowPolicy::bounded(80),
        key: ProjectionKey::app_owned(key).unwrap(),
        item_projection: FeedItemProjection::FeedRows,
    }
}

fn has_h(filter: &serde_json::Value, local_id: &str) -> bool {
    filter
        .get("#h")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(local_id)))
}

#[test]
fn active_hosted_groups_flat_rows_include_matched_group_context() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let viewer = keys_from_byte(70);
    let author = keys_from_byte(71);
    let viewer_pk = viewer.public_key().to_hex();

    let mut relay = RecordingRelay::spawn(Vec::new());
    let relay_url = relay.url().to_string();
    let simple_groups = signed_simple_groups(&viewer, &[("room-a", &relay_url)], 100);
    let group_message = signed_group_message(&author, "room-a", "hosted group row", 110);
    let group_message_id = group_message.id.to_hex();

    let app = new_started_reduced_source_app();
    add_relay(app, &relay_url);
    sign_in(app, &viewer);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &viewer_pk);

    let key = "test.relay.hosted-groups.context";
    let _handle = app_ref
        .open_feed(&hosted_groups_params(key))
        .expect("ActiveUserHostedGroups flat feed opens");

    relay.wait_req("active simple-groups source", |filter| {
        has_kind(filter, 10_009) && has_author(filter, &viewer_pk)
    });
    relay.push_event(simple_groups);
    relay.wait_req("host-pinned group message source", |filter| {
        has_kind(filter, 9) && has_h(filter, "room-a")
    });
    relay.push_event(group_message);

    wait_for(&rx, "hosted group context row", || {
        flat_feed_cards(app_ref, key).iter().any(|card| {
            card.id == group_message_id
                && card.hosted_group.as_ref().is_some_and(|context| {
                    context.host_relay_url == relay_url && context.local_id == "room-a"
                })
        })
    });

    let cards = flat_feed_cards(app_ref, key);
    let card = cards
        .iter()
        .find(|card| card.id == group_message_id)
        .expect("group message card");
    assert_eq!(
        card.hosted_group,
        Some(nmp_note_feed::HostedGroupContext {
            host_relay_url: relay_url,
            local_id: "room-a".to_string(),
        })
    );

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
