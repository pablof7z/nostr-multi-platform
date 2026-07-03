use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::ObservedProjection;
use nmp_core::ObservedProjectionId;
use nmp_nip29::GroupId;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, ReadSessionRegistry,
    TeardownAction,
};

use super::*;

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room-a")
}

fn group_with(local_id: &str) -> GroupId {
    GroupId::new("wss://groups.example.com", local_id)
}

fn session(local_id: &str) -> ChatPresenceSession {
    ChatPresenceSession::new(group_with(local_id), "me".into(), vec![9])
}

#[test]
fn filter_json_sorts_and_dedups_caller_owned_kinds() {
    let v: serde_json::Value =
        serde_json::from_str(&chat_presence_filter_json(&group(), &[11, 9, 9])).unwrap();
    assert_eq!(v["kinds"], serde_json::json!([9, 11]));
    assert_eq!(v["#h"], serde_json::json!(["room-a"]));
    assert!(v.get("relay_pin").is_none());
}

#[test]
fn empty_kinds_means_all_group_events() {
    let v: serde_json::Value =
        serde_json::from_str(&chat_presence_filter_json(&group(), &[])).unwrap();
    assert!(v.get("kinds").is_none());
    assert_eq!(v["#h"], serde_json::json!(["room-a"]));
}

#[test]
fn filter_json_includes_remote_typing_kinds_when_messages_are_bounded() {
    let v: serde_json::Value = serde_json::from_str(&chat_presence_filter_json_with_remote_typing(
        &group(),
        &[11, 9, 9],
        &[24_010, 9],
    ))
    .unwrap();
    assert_eq!(v["kinds"], serde_json::json!([9, 11, 24010]));
    assert_eq!(v["#h"], serde_json::json!(["room-a"]));
}

#[test]
fn session_descriptor_exposes_scope_without_kind_policy() {
    let session = ChatPresenceSession::new(group(), "me".into(), vec![11, 9]);
    assert_eq!(session.group_id().local_id, "room-a");
    assert_eq!(session.active_pubkey(), "me");
    assert_eq!(session.message_kinds(), &[11, 9]);
    assert!(session.remote_typing().kinds().is_empty());
    assert_eq!(
        session.projection_key(),
        chat_presence_projection_key(&group())
    );
}

#[test]
fn projection_key_uses_group_identity_family() {
    let key = chat_presence_projection_key(&group());

    assert_eq!(
        key,
        "nmp.chat.presence.h7773733a2f2f67726f7570732e6578616d706c652e636f6d.g726f6f6d2d61"
    );
    assert_ne!(key, CHAT_PRESENCE_KEY);
    assert!(key.starts_with("nmp.chat.presence."));
    assert!(!key.contains("wss://"));
}

#[derive(Default)]
struct FakeHost {
    registry: ReadSessionRegistry,
    installed_keys: Mutex<Vec<String>>,
    emitted_keys: Mutex<Vec<String>>,
    opened_filters: Mutex<Vec<String>>,
    opened_consumers: Mutex<Vec<String>>,
    closed_outputs: Arc<Mutex<Vec<String>>>,
    closed_interests: Arc<Mutex<Vec<u64>>>,
    next_interest: AtomicU64,
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        self.installed_keys
            .lock()
            .unwrap()
            .push(key.as_str().to_string());
        self.emitted_keys
            .lock()
            .unwrap()
            .push(encoder().expect("chat presence emits").key);
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.opened_filters.lock().unwrap().push(decl.filter_json);
        self.opened_consumers.lock().unwrap().push(decl.consumer_id);
        ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let closed = Arc::clone(&self.closed_interests);
        Box::new(move || closed.lock().unwrap().push(id.0))
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        let closed = Arc::clone(&self.closed_outputs);
        Box::new(move || closed.lock().unwrap().push(key))
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        Box::new(|| {})
    }

    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.registry.open(build)
    }

    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.registry.projection_key(id)
    }

    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.registry.close(id)
    }

    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.registry.close_by_projection_key(projection_key)
    }
}

#[test]
fn sessions_for_distinct_groups_stay_concurrent() {
    let host = FakeHost::default();
    let (room_a, _) = open_chat_presence_session_with_reader(&host, session("room-a"));
    let (room_b, _) = open_chat_presence_session_with_reader(&host, session("room-b"));

    assert_ne!(room_a.key(), room_b.key());
    assert_eq!(host.registry.live_count(), 2, "both room reads stay live");
    assert!(
        host.closed_outputs.lock().unwrap().is_empty(),
        "opening another room must not close the prior room output"
    );
    assert_eq!(
        host.installed_keys.lock().unwrap().clone(),
        vec![room_a.key().to_string(), room_b.key().to_string()]
    );
    assert_eq!(
        host.emitted_keys.lock().unwrap().clone(),
        vec![room_a.key().to_string(), room_b.key().to_string()],
        "typed sidecars emit under the concrete room keys"
    );
    assert_eq!(
        host.opened_consumers.lock().unwrap().clone(),
        vec![room_a.key().to_string(), room_b.key().to_string()],
        "observed interests use the concrete room key as their owner"
    );
}

#[test]
fn reopening_same_group_replaces_only_that_group() {
    let host = FakeHost::default();
    let (first_room_a, _) = open_chat_presence_session_with_reader(&host, session("room-a"));
    let (room_b, _) = open_chat_presence_session_with_reader(&host, session("room-b"));
    let (replacement_room_a, _) = open_chat_presence_session_with_reader(&host, session("room-a"));

    assert_eq!(first_room_a.key(), replacement_room_a.key());
    assert_ne!(replacement_room_a.key(), room_b.key());
    assert_eq!(
        host.registry.live_count(),
        2,
        "same-room replacement keeps room-b live"
    );
    assert_eq!(
        host.closed_outputs.lock().unwrap().clone(),
        vec![first_room_a.key().to_string()],
        "only room-a was tombstoned before replacement"
    );
    assert_eq!(
        host.closed_interests.lock().unwrap().len(),
        1,
        "only room-a's previous interest was withdrawn"
    );

    assert!(
        !close_chat_presence_session(&host, first_room_a),
        "stale replaced handle cannot close the replacement"
    );
    assert!(close_chat_presence_session(&host, replacement_room_a));
    assert!(close_chat_presence_session(&host, room_b));
    assert_eq!(host.registry.live_count(), 0);
}

#[test]
fn session_remote_typing_spec_extends_opened_filter() {
    let host = FakeHost::default();
    let descriptor = session("room-a").with_remote_typing(ChatRemoteTypingSpec::new(vec![24_010]));
    let (handle, _) = open_chat_presence_session_with_reader(&host, descriptor);

    assert_eq!(handle.key(), chat_presence_projection_key(&group()));
    let filters = host.opened_filters.lock().unwrap();
    let v: serde_json::Value = serde_json::from_str(&filters[0]).unwrap();
    assert_eq!(v["kinds"], serde_json::json!([9, 24010]));
    assert_eq!(v["#h"], serde_json::json!(["room-a"]));
}
