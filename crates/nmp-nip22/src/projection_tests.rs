use super::*;

use nmp_kinds::KIND_NIP22_COMMENT;

const ROOT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_ROOT: &str = "9999999999999999999999999999999999999999999999999999999999999999";

fn record(event_id: &str, parent_value: &str, created_at: u64) -> CommentRecord {
    CommentRecord {
        event_id: event_id.to_string(),
        author_pubkey: "author".to_string(),
        body: String::new(),
        root_tag_name: "E".to_string(),
        root_tag_value: ROOT.to_string(),
        root_kind: "11".to_string(),
        root_author_pubkey: String::new(),
        parent_tag_name: "e".to_string(),
        parent_tag_value: parent_value.to_string(),
        parent_kind: "1111".to_string(),
        created_at,
    }
}

fn comment_kernel_event(event_id: &str, parent_value: &str, root: &str) -> KernelEvent {
    KernelEvent {
        id: event_id.to_string(),
        author: "author".to_string(),
        kind: KIND_NIP22_COMMENT,
        created_at: 1,
        tags: vec![
            vec!["E".to_string(), root.to_string()],
            vec!["K".to_string(), "11".to_string()],
            vec!["e".to_string(), parent_value.to_string()],
            vec!["k".to_string(), "1111".to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn build_thread_orders_children_and_promotes_orphans() {
    let tree = build_thread(
        &[
            record("newer-top", ROOT, 30),
            record("newer-child", "older-top", 25),
            record("older-child", "older-top", 20),
            record("orphan", "missing-parent", 15),
            record("older-top", ROOT, 10),
        ],
        ROOT,
    );

    // Top level is oldest-first, with the orphan (missing parent) promoted.
    assert_eq!(
        tree.iter()
            .map(|node| node.record.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["older-top", "newer-top", "orphan"]
    );
    // Children oldest-first.
    assert_eq!(
        tree[0]
            .children
            .iter()
            .map(|node| node.record.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["older-child", "newer-child"]
    );
}

#[test]
fn build_thread_breaks_self_referential_parent_edges() {
    let mut self_child = record("self-child", "top", 2);
    self_child.parent_tag_value = self_child.event_id.clone();

    let tree = build_thread(&[record("top", ROOT, 1), self_child], ROOT);

    assert_eq!(tree.len(), 1);
    assert!(tree[0].children.is_empty());
}

#[test]
fn projection_buckets_by_root_and_builds_tree() {
    let projection = CommentThreadProjection::new();
    projection.on_kernel_event(&comment_kernel_event("top", ROOT, ROOT));
    projection.on_kernel_event(&comment_kernel_event("reply", "top", ROOT));
    // A comment on a different root must not leak into this thread.
    projection.on_kernel_event(&comment_kernel_event("elsewhere", OTHER_ROOT, OTHER_ROOT));

    let snapshot = projection.snapshot_for(ROOT);

    assert_eq!(snapshot.root_tag_value, ROOT);
    assert_eq!(snapshot.records.len(), 2);
    assert_eq!(snapshot.tree.len(), 1);
    assert_eq!(snapshot.tree[0].record.event_id, "top");
    assert_eq!(snapshot.tree[0].children[0].record.event_id, "reply");
}

#[test]
fn projection_ignores_non_comment_events() {
    let projection = CommentThreadProjection::new();
    let not_a_comment = KernelEvent {
        kind: 1,
        ..comment_kernel_event("note", ROOT, ROOT)
    };
    projection.on_kernel_event(&not_a_comment);

    assert!(projection.snapshot_for(ROOT).records.is_empty());
}

#[test]
fn snapshot_for_unknown_root_is_empty() {
    let projection = CommentThreadProjection::new();
    let snapshot = projection.snapshot_for(ROOT);
    assert_eq!(snapshot.root_tag_value, ROOT);
    assert!(snapshot.records.is_empty());
    assert!(snapshot.tree.is_empty());
}
