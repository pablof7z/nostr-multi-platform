use super::*;

const VIEWER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn event(id: &str, kind: u32, tags: Vec<Vec<&str>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: ACTOR.to_string(),
        kind,
        created_at: 42,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: content.to_string(),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

#[test]
fn captures_reply_to_viewer_with_source_relay() {
    let projection = NotificationsProjection::new(VIEWER.to_string());
    projection.on_kernel_event(&event(
        "reply",
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["e", TARGET], vec!["p", VIEWER]],
        "reply body",
    ));

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].notification_kind, NotificationKind::Reply);
    assert_eq!(snapshot.rows[0].target_event_id.as_deref(), Some(TARGET));
    assert_eq!(snapshot.rows[0].source_relays, vec!["wss://relay.example"]);
    assert_eq!(snapshot.unread_count, 1);
}

#[test]
fn ignores_self_authored_and_unaddressed_events() {
    let projection = NotificationsProjection::new(VIEWER.to_string());
    let mut self_event = event("self", KIND_REACTION, vec![vec!["p", VIEWER]], "+");
    self_event.author = VIEWER.to_string();
    projection.on_kernel_event(&self_event);
    projection.on_kernel_event(&event("other", KIND_REACTION, vec![], "+"));
    assert!(projection.snapshot().rows.is_empty());
}

#[test]
fn mark_read_updates_snapshot_without_accepting_unknown_ids() {
    let projection = NotificationsProjection::new(VIEWER.to_string());
    projection.on_kernel_event(&event(
        "reply",
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["e", TARGET], vec!["p", VIEWER]],
        "reply body",
    ));

    assert_eq!(projection.mark_read(["unknown", "reply"]), 1);
    let snapshot = projection.snapshot();
    assert_eq!(snapshot.unread_count, 0);
    assert!(snapshot.rows[0].read);
}

#[test]
fn mark_all_read_is_idempotent() {
    let projection = NotificationsProjection::new(VIEWER.to_string());
    projection.on_kernel_event(&event(
        "reply",
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["e", TARGET], vec!["p", VIEWER]],
        "reply body",
    ));
    projection.on_kernel_event(&event(
        "mention",
        KIND_SHORT_TEXT_NOTE,
        vec![vec!["p", VIEWER]],
        "hi",
    ));

    assert_eq!(projection.mark_all_read(), 2);
    assert_eq!(projection.mark_all_read(), 0);
    assert_eq!(projection.snapshot().unread_count, 0);
}

#[test]
fn interest_shape_is_bounded_p_tag_inbox() {
    let shape = notifications_interest_shape(VIEWER);
    assert_eq!(shape.limit, Some(NOTIFICATIONS_LIMIT));
    assert!(shape.kinds.contains(&KIND_REACTION));
    assert!(shape
        .tags
        .get("p")
        .is_some_and(|values| values.contains(VIEWER)));
}
