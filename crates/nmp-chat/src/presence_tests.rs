use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;

use crate::typing_event::{chat_typing_status_tag, ChatRemoteTypingSpec};

use super::*;

fn projection() -> ChatPresenceProjection {
    ChatPresenceProjection::new("wss://groups.example.com", "room-a", "me", vec![9, 11])
}

fn event(id: &str, author: &str, kind: u32, created_at: u64, group: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind,
        created_at,
        tags: vec![vec!["h".into(), group.into()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn typing_event(
    id: &str,
    author: &str,
    created_at: u64,
    group: &str,
    is_typing: bool,
) -> KernelEvent {
    KernelEvent {
        tags: vec![
            vec!["h".into(), group.into()],
            chat_typing_status_tag(is_typing),
        ],
        ..event(id, author, 24_010, created_at, group)
    }
}

#[test]
fn unread_counts_non_self_messages_newer_than_read_marker() {
    let p = projection();
    p.on_kernel_event(&event("a", "alice", 9, 10, "room-a"));
    p.on_kernel_event(&event("b", "me", 9, 11, "room-a"));
    p.on_kernel_event(&event("c", "carol", 11, 12, "room-a"));
    p.on_kernel_event(&event("d", "dan", 1111, 13, "room-a"));
    p.on_kernel_event(&event("e", "erin", 9, 14, "other"));

    assert_eq!(p.snapshot().unread_count, 2);
    assert!(p.mark_read(ReadMarker::new("a", 10)));
    assert_eq!(p.snapshot().unread_count, 1);
    assert!(p.mark_read(ReadMarker::new("c", 12)));
    assert_eq!(p.snapshot().unread_count, 0);
}

#[test]
fn read_marker_advances_monotonically() {
    let p = projection();
    assert!(p.mark_read(ReadMarker::new("b", 10)));
    assert!(!p.mark_read(ReadMarker::new("a", 10)));
    assert_eq!(p.snapshot().read_marker, Some(ReadMarker::new("b", 10)));
    assert!(p.mark_read(ReadMarker::new("c", 10)));
    assert_eq!(p.snapshot().read_marker, Some(ReadMarker::new("c", 10)));
}

#[test]
fn typing_state_is_explicit_and_excludes_self() {
    let p = projection();
    assert!(p.apply_typing(TypingUpdate::started("alice", 100, 200)));
    assert!(!p.apply_typing(TypingUpdate::started("me", 100, 200)));
    assert_eq!(p.snapshot().typing.len(), 1);

    assert!(!p.advance_clock(199));
    assert_eq!(p.snapshot().typing[0].pubkey, "alice");
    assert!(p.advance_clock(200));
    assert!(p.snapshot().typing.is_empty());
}

#[test]
fn stopped_typing_removes_participant() {
    let p = projection();
    assert!(p.apply_typing(TypingUpdate::started("alice", 100, 300)));
    assert!(p.apply_typing(TypingUpdate::stopped("alice", 150)));
    assert!(p.snapshot().typing.is_empty());
}

#[test]
fn remote_typing_events_update_typing_state() {
    let p = ChatPresenceProjection::with_remote_typing(
        "wss://groups.example.com",
        "room-a",
        "me",
        vec![9],
        ChatRemoteTypingSpec::with_ttl_ms(vec![24_010], 5_000),
    );

    p.on_kernel_event(&typing_event("t1", "alice", 10, "room-a", true));
    assert_eq!(
        p.snapshot().typing,
        vec![ChatPresenceTyping {
            pubkey: "alice".into(),
            updated_at_ms: 10_000,
            expires_at_ms: 15_000,
        }]
    );

    p.on_kernel_event(&typing_event("t2", "alice", 11, "room-a", false));
    assert!(p.snapshot().typing.is_empty());
}

#[test]
fn remote_typing_excludes_self_and_non_matching_groups() {
    let p = ChatPresenceProjection::with_remote_typing(
        "wss://groups.example.com",
        "room-a",
        "me",
        vec![9],
        ChatRemoteTypingSpec::new(vec![24_010]),
    );

    p.on_kernel_event(&typing_event("t1", "me", 10, "room-a", true));
    p.on_kernel_event(&typing_event("t2", "alice", 10, "room-b", true));

    assert!(p.snapshot().typing.is_empty());
}

#[test]
fn remote_typing_kind_does_not_increment_unread_when_messages_are_unbounded() {
    let p = ChatPresenceProjection::with_remote_typing(
        "wss://groups.example.com",
        "room-a",
        "me",
        Vec::new(),
        ChatRemoteTypingSpec::new(vec![24_010]),
    );

    p.on_kernel_event(&typing_event("t1", "alice", 10, "room-a", true));
    p.on_kernel_event(&event("m1", "alice", 9, 11, "room-a"));

    assert_eq!(p.snapshot().unread_count, 1);
    assert_eq!(p.snapshot().typing.len(), 1);
}
