use super::*;
use nmp_core::KernelEventObserver;
use std::sync::Arc;

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room")
}

fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: format!("author-{id}"),
        kind,
        created_at,
        tags,
        content: format!("content-{id}"),
        relay_provenance: vec!["wss://groups.example.com".into()],
    }
}

fn h(local_id: &str) -> Vec<Vec<String>> {
    vec![vec!["h".into(), local_id.into()]]
}

#[test]
fn fresh_projection_yields_empty_snapshot_with_group_identity() {
    let projection = GroupEventsProjection::new(group());
    assert_eq!(
        projection.snapshot(),
        GroupEventsSnapshot {
            group_id: "room".into(),
            host_relay_url: "wss://groups.example.com".into(),
            events: Vec::new(),
        }
    );
}

#[test]
fn matching_h_tagged_event_preserves_raw_fields_tags_and_provenance() {
    let projection = GroupEventsProjection::new(group());
    projection.on_kernel_event(&event(
        "e1",
        16,
        100,
        vec![
            vec!["h".into(), "room".into()],
            vec![
                "e".into(),
                "target".into(),
                "wss://relay".into(),
                "mention".into(),
            ],
            vec!["t".into(), "nostr".into()],
        ],
    ));

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.events.len(), 1);
    let row = &snapshot.events[0];
    assert_eq!(row.id, "e1");
    assert_eq!(row.pubkey, "author-e1");
    assert_eq!(row.kind, 16);
    assert_eq!(row.content, "content-e1");
    assert_eq!(row.created_at, 100);
    assert_eq!(
        row.tags,
        vec![
            vec!["h".to_string(), "room".to_string()],
            vec![
                "e".to_string(),
                "target".to_string(),
                "wss://relay".to_string(),
                "mention".to_string()
            ],
            vec!["t".to_string(), "nostr".to_string()],
        ]
    );
    assert_eq!(row.relay_provenance, vec!["wss://groups.example.com"]);
}

#[test]
fn different_or_missing_h_tag_is_excluded() {
    let projection = GroupEventsProjection::new(group());
    projection.on_kernel_event(&event("other", 16, 10, h("other-room")));
    projection.on_kernel_event(&event("missing", 16, 11, Vec::new()));
    assert!(projection.snapshot().events.is_empty());
}

#[test]
fn any_kind_with_matching_h_tag_is_retained() {
    let projection = GroupEventsProjection::new(group());
    projection.on_kernel_event(&event("note", 1, 10, h("room")));
    projection.on_kernel_event(&event("future", 40000, 11, h("room")));

    let kinds: Vec<u32> = projection
        .snapshot()
        .events
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![40000, 1]);
}

#[test]
fn events_are_deduped_and_ordered_newest_first() {
    let projection = GroupEventsProjection::new(group());
    projection.on_kernel_event(&event("old", 16, 10, h("room")));
    projection.on_kernel_event(&event("new", 16, 20, h("room")));
    projection.on_kernel_event(&event("new", 16, 20, h("room")));

    let snapshot = projection.snapshot();
    let ids: Vec<&str> = snapshot.events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["new", "old"]);
}

#[test]
fn snapshot_json_uses_raw_event_shape() {
    let projection = GroupEventsProjection::new(group());
    projection.on_kernel_event(&event("e1", 11, 10, h("room")));

    let json = projection.snapshot_json();
    assert_eq!(json["group_id"], "room");
    assert_eq!(json["host_relay_url"], "wss://groups.example.com");
    assert_eq!(json["events"][0]["id"], "e1");
    assert_eq!(json["events"][0]["tags"][0][0], "h");
}

#[test]
fn drives_through_observer_trait_object() {
    let projection = Arc::new(GroupEventsProjection::new(group()));
    let observer: Arc<dyn KernelEventObserver> = Arc::clone(&projection) as _;
    observer.on_kernel_event(&event("e1", 16, 10, h("room")));
    assert_eq!(projection.snapshot().events.len(), 1);
}
