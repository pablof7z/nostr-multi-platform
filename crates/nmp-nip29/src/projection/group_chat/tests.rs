use super::*;
use std::sync::Arc;

/// The group every test event in this module targets.
fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "rust-nostr")
}

/// Build a `KernelEvent` with an explicit kind and tag set.
fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: format!("author-of-{id}"),
        kind,
        created_at,
        tags,
        content: format!("content of {id}"),
    }
}

/// `["h", local_id]` for the test group.
fn h_tag(local_id: &str) -> Vec<Vec<String>> {
    vec![vec!["h".into(), local_id.into()]]
}

#[test]
fn fresh_projection_yields_empty_snapshot() {
    let proj = GroupChatProjection::new(group());
    let snap = proj.snapshot();
    assert!(snap.messages.is_empty());
    let json = proj.snapshot_json();
    assert_eq!(json, serde_json::json!({ "messages": [] }));
}

#[test]
fn matching_chat_message_is_retained() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1);
    let msg = &snap.messages[0];
    assert_eq!(msg.id, "e1");
    assert_eq!(msg.pubkey, "author-of-e1");
    assert_eq!(msg.content, "content of e1");
    assert_eq!(msg.created_at, 100);
    assert_eq!(msg.kind, KIND_CHAT_MESSAGE);
}

#[test]
fn thread_kind_is_retained() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event(
        "thread",
        KIND_DISCUSSION_OR_ARTIFACT,
        10,
        h_tag("rust-nostr"),
    ));

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1);
    let kinds: Vec<u32> = snap.messages.iter().map(|m| m.kind).collect();
    assert!(kinds.contains(&KIND_DISCUSSION_OR_ARTIFACT));
}

#[test]
fn event_for_a_different_group_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // Correct kind, but the `h` tag names a different group.
    proj.on_kernel_event(&event("other", KIND_CHAT_MESSAGE, 100, h_tag("some-other-room")));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn event_with_no_h_tag_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // Correct kind, but no `h` tag at all — not a group event.
    proj.on_kernel_event(&event("loose", KIND_CHAT_MESSAGE, 100, vec![]));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn off_kind_event_with_matching_h_tag_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // kind 1 (plain note) and kind 9000 (a moderation action) both carry a
    // matching `h` tag, but neither is a chat-content kind.
    proj.on_kernel_event(&event("note", 1, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("modaction", 9000, 100, h_tag("rust-nostr")));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn messages_are_ordered_newest_first() {
    let proj = GroupChatProjection::new(group());
    // Deliver out of chronological order.
    proj.on_kernel_event(&event("mid", KIND_CHAT_MESSAGE, 200, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("old", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("new", KIND_CHAT_MESSAGE, 300, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

#[test]
fn created_at_ties_break_on_id_descending() {
    let proj = GroupChatProjection::new(group());
    // Same `created_at` — order must still be total and deterministic.
    proj.on_kernel_event(&event("aaa", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("zzz", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["zzz", "aaa"]);
}

#[test]
fn duplicate_event_id_is_not_duplicated() {
    let proj = GroupChatProjection::new(group());
    let evt = event("dup", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr"));
    proj.on_kernel_event(&evt);
    proj.on_kernel_event(&evt);

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1, "re-delivered id must not duplicate");
}

#[test]
fn snapshot_json_contains_the_messages() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event(
        "e2",
        KIND_DISCUSSION_OR_ARTIFACT,
        200,
        h_tag("rust-nostr"),
    ));

    let json = proj.snapshot_json();
    let messages = json
        .get("messages")
        .and_then(|m| m.as_array())
        .expect("snapshot json has a `messages` array");
    assert_eq!(messages.len(), 2);
    // Newest-first: e2 (created_at 200) precedes e1 (created_at 100).
    assert_eq!(messages[0].get("id").and_then(|v| v.as_str()), Some("e2"));
    assert_eq!(messages[1].get("id").and_then(|v| v.as_str()), Some("e1"));
    // Field shape: `pubkey` carries `KernelEvent::author`.
    assert_eq!(
        messages[0].get("pubkey").and_then(|v| v.as_str()),
        Some("author-of-e2"),
    );
}

#[test]
fn round_trips_through_serde() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: GroupChatSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(snap, decoded);
}

#[test]
fn drives_through_observer_trait_object() {
    // The projection must be usable as `Arc<dyn KernelEventObserver>` —
    // that is exactly how a host registers it with `register_event_observer`.
    let proj = Arc::new(GroupChatProjection::new(group()));
    let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
    observer.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    assert_eq!(proj.snapshot().messages.len(), 1);
}

#[test]
fn group_id_accessor_returns_construction_value() {
    let proj = GroupChatProjection::new(group());
    assert_eq!(proj.group_id(), &group());
}

#[test]
fn empty_snapshot_constructor_yields_no_messages() {
    let empty = GroupChatSnapshot::empty();
    assert!(empty.messages.is_empty());
}
