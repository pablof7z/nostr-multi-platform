use super::*;
use crate::group_id::GroupId;
use crate::group_query::{GroupEventKinds, GroupEventsQuery};
use std::sync::Arc;

// kind:9 (chat) and kind:11 (thread) are caller-declared kinds; the projection
// is kind-agnostic, so the tests pass literal kind numbers — NIP-29 owns no
// constant for these foreign kinds.

/// The group every test event in this module targets.
fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "rust-nostr")
}

/// A chat-scoped query (kinds {9, 11}) for the test group.
fn chat_query() -> GroupEventsQuery {
    GroupEventsQuery::from_kinds(group(), vec![9, 11])
}

/// An all-kinds query for the test group.
fn all_query() -> GroupEventsQuery {
    GroupEventsQuery::new(group(), GroupEventKinds::All)
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
        relay_provenance: Vec::new(),
    }
}

/// `["h", local_id]` for the test group.
fn h_tag(local_id: &str) -> Vec<Vec<String>> {
    vec![vec!["h".into(), local_id.into()]]
}

#[test]
fn fresh_projection_yields_empty_snapshot() {
    let proj = GroupEventsProjection::new(chat_query());
    let snap = proj.snapshot();
    assert!(snap.events.is_empty());
    let json = proj.snapshot_json();
    assert_eq!(json, serde_json::json!({ "events": [] }));
}

#[test]
fn matching_chat_event_is_retained() {
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    assert_eq!(snap.events.len(), 1);
    let msg = &snap.events[0];
    assert_eq!(msg.id, "e1");
    assert_eq!(msg.pubkey, "author-of-e1");
    assert_eq!(msg.content, "content of e1");
    assert_eq!(msg.created_at, 100);
    assert_eq!(msg.kind, 9);
}

#[test]
fn thread_kind_is_retained() {
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("thread", 11, 10, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    assert_eq!(snap.events.len(), 1);
    let kinds: Vec<u32> = snap.events.iter().map(|m| m.kind).collect();
    assert!(kinds.contains(&11));
}

#[test]
fn all_kinds_query_retains_any_h_tagged_kind() {
    // The whole point of #2187: a kind-agnostic group read. An `All` query
    // accepts a kind:1111 comment, a kind:9 chat, and a future kind:40000 alike,
    // as long as they carry the matching `h` tag.
    let proj = GroupEventsProjection::new(all_query());
    proj.on_kernel_event(&event("chat", 9, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("comment", 1111, 110, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("future", 40000, 120, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let kinds: Vec<u32> = snap.events.iter().map(|m| m.kind).collect();
    assert_eq!(snap.events.len(), 3);
    assert!(kinds.contains(&9));
    assert!(kinds.contains(&1111));
    assert!(kinds.contains(&40000));
}

#[test]
fn specific_query_kind_gates_other_same_h_kinds() {
    // A chat query ({9, 11}) MUST reject a same-`h` kind:1111 comment even when
    // it is delivered (cache replay / store hydration can deliver kinds the wire
    // filter did not request — the projection is the second gate).
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("chat", 9, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("comment", 1111, 110, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.events.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["chat"]);
}

#[test]
fn event_for_a_different_group_is_excluded() {
    let proj = GroupEventsProjection::new(chat_query());
    // Correct kind, but the `h` tag names a different group.
    proj.on_kernel_event(&event("other", 9, 100, h_tag("some-other-room")));
    assert!(proj.snapshot().events.is_empty());
}

#[test]
fn event_with_no_h_tag_is_excluded_even_under_all_kinds() {
    // h-tag-only rejection: an event with no `h` tag is never a group event, no
    // matter how permissive the kind selection.
    let proj = GroupEventsProjection::new(all_query());
    proj.on_kernel_event(&event("loose", 9, 100, vec![]));
    proj.on_kernel_event(&event("loose2", 1, 100, vec![]));
    assert!(proj.snapshot().events.is_empty());
}

#[test]
fn off_kind_event_with_matching_h_tag_is_excluded() {
    let proj = GroupEventsProjection::new(chat_query());
    // kind 1 (plain note) and kind 9000 (a moderation action) both carry a
    // matching `h` tag, but neither is in the {9, 11} chat selection.
    proj.on_kernel_event(&event("note", 1, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("modaction", 9000, 100, h_tag("rust-nostr")));
    assert!(proj.snapshot().events.is_empty());
}

#[test]
fn events_are_ordered_newest_first() {
    let proj = GroupEventsProjection::new(chat_query());
    // Deliver out of chronological order.
    proj.on_kernel_event(&event("mid", 9, 200, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("old", 9, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("new", 9, 300, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.events.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

#[test]
fn created_at_ties_break_on_id_descending() {
    let proj = GroupEventsProjection::new(chat_query());
    // Same `created_at` — order must still be total and deterministic.
    proj.on_kernel_event(&event("aaa", 9, 500, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("zzz", 9, 500, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.events.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["zzz", "aaa"]);
}

#[test]
fn duplicate_event_id_is_not_duplicated() {
    let proj = GroupEventsProjection::new(chat_query());
    let evt = event("dup", 9, 100, h_tag("rust-nostr"));
    proj.on_kernel_event(&evt);
    proj.on_kernel_event(&evt);

    let snap = proj.snapshot();
    assert_eq!(snap.events.len(), 1, "re-delivered id must not duplicate");
}

#[test]
fn snapshot_json_contains_the_events() {
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("e2", 11, 200, h_tag("rust-nostr")));

    let json = proj.snapshot_json();
    let events = json
        .get("events")
        .and_then(|m| m.as_array())
        .expect("snapshot json has a `events` array");
    assert_eq!(events.len(), 2);
    // Newest-first: e2 (created_at 200) precedes e1 (created_at 100).
    assert_eq!(events[0].get("id").and_then(|v| v.as_str()), Some("e2"));
    assert_eq!(events[1].get("id").and_then(|v| v.as_str()), Some("e1"));
    // Field shape: `pubkey` carries `KernelEvent::author`.
    assert_eq!(
        events[0].get("pubkey").and_then(|v| v.as_str()),
        Some("author-of-e2"),
    );
    // Minimal snapshot: no group id / kinds echoed.
    assert!(json.get("group").is_none());
    assert!(json.get("kinds").is_none());
}

#[test]
fn round_trips_through_serde() {
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: GroupEventsSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(snap, decoded);
}

#[test]
fn drives_through_observer_trait_object() {
    // The projection must be usable as `Arc<dyn ObservedProjectionSink>` —
    // that is exactly how a host passes it into an observed projection.
    let proj = Arc::new(GroupEventsProjection::new(chat_query()));
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&proj) as _;
    observer.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));
    assert_eq!(proj.snapshot().events.len(), 1);
}

#[test]
fn query_accessor_returns_construction_value() {
    let proj = GroupEventsProjection::new(chat_query());
    assert_eq!(proj.query(), &chat_query());
}

#[test]
fn poisoned_mutex_yields_empty_snapshot() {
    // D6: a poisoned internal mutex degrades to an empty snapshot rather than
    // panicking on the actor thread.
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));
    let proj = Arc::new(proj);
    let poisoner = Arc::clone(&proj);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.events.lock().unwrap();
        panic!("poison the mutex");
    })
    .join();
    assert!(proj.events.is_poisoned());
    assert_eq!(proj.snapshot(), GroupEventsSnapshot::empty());
    assert_eq!(proj.snapshot_json(), serde_json::json!({ "events": [] }));
}

#[test]
fn empty_snapshot_constructor_yields_no_events() {
    let empty = GroupEventsSnapshot::empty();
    assert!(empty.events.is_empty());
}

#[test]
fn standalone_chat_event_has_no_reply_edges() {
    let proj = GroupEventsProjection::new(chat_query());
    proj.on_kernel_event(&event("e1", 9, 100, h_tag("rust-nostr")));
    let snap = proj.snapshot();
    let row = &snap.events[0];
    assert_eq!(row.reply_to, None);
    assert_eq!(row.root, None);
    // Absent edges are omitted from the JSON fallback shape.
    let json = proj.snapshot_json();
    let event_json = &json["events"][0];
    assert!(event_json.get("reply_to").is_none());
    assert!(event_json.get("root").is_none());
}

#[test]
fn marked_reply_surfaces_parent_and_root() {
    let proj = GroupEventsProjection::new(chat_query());
    let tags = vec![
        vec!["h".into(), "rust-nostr".into()],
        vec!["e".into(), "rootid".into(), String::new(), "root".into()],
        vec!["e".into(), "parentid".into(), String::new(), "reply".into()],
    ];
    proj.on_kernel_event(&event("e1", 9, 100, tags));
    let snap = proj.snapshot();
    let row = &snap.events[0];
    assert_eq!(row.reply_to.as_deref(), Some("parentid"));
    assert_eq!(row.root.as_deref(), Some("rootid"));
}

#[test]
fn positional_thread_event_uses_first_root_last_parent() {
    let proj = GroupEventsProjection::new(all_query());
    let tags = vec![
        vec!["h".into(), "rust-nostr".into()],
        vec!["e".into(), "rootid".into()],
        vec!["e".into(), "parentid".into()],
    ];
    proj.on_kernel_event(&event("e1", 11, 100, tags));
    let snap = proj.snapshot();
    let row = &snap.events[0];
    assert_eq!(row.root.as_deref(), Some("rootid"));
    assert_eq!(row.reply_to.as_deref(), Some("parentid"));
}
