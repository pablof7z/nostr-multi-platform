use nmp_nip29::GroupId;

use super::*;

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room-a")
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
fn session_descriptor_exposes_scope_without_kind_policy() {
    let session = ChatPresenceSession::new(group(), "me".into(), vec![11, 9]);
    assert_eq!(session.group_id().local_id, "room-a");
    assert_eq!(session.active_pubkey(), "me");
    assert_eq!(session.message_kinds(), &[11, 9]);
}
