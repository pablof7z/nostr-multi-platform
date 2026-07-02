use super::*;
use nmp_core::substrate::KernelEvent;

fn event(id: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "author".to_string(),
        kind: 1,
        created_at: 100,
        tags: tags
            .into_iter()
            .map(|t| t.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn root_event_has_no_parent_edge() {
    let projection = ThreadingProjection::etag(ModulePolicy::default());
    let root = event(&"a".repeat(64), vec![]);
    projection.on_kernel_event(&root);

    let snapshot = projection.snapshot();
    assert_eq!(snapshot.edges.len(), 1);
    assert!(snapshot.edges[0].parent.is_none());
    assert!(snapshot.edges[0].root.is_none());
}

#[test]
fn reply_resolves_parent_and_root_pointers() {
    let projection = ThreadingProjection::etag(ModulePolicy::default());
    let root_id = "a".repeat(64);
    let reply_id = "b".repeat(64);

    projection.on_kernel_event(&event(&root_id, vec![]));
    projection.on_kernel_event(&event(
        &reply_id,
        vec![
            vec!["e", &root_id, "", "root"],
            vec!["e", &root_id, "", "reply"],
        ],
    ));

    let snapshot = projection.snapshot();
    let reply_edge = snapshot
        .edges
        .iter()
        .find(|e| e.event_id == reply_id)
        .expect("reply edge present");
    assert_eq!(
        reply_edge.parent.as_ref().unwrap().event_id(),
        Some(root_id.as_str())
    );
    assert_eq!(
        reply_edge.root.as_ref().unwrap().event_id(),
        Some(root_id.as_str())
    );
}

#[test]
fn grouper_collapses_reply_into_a_module_block() {
    let projection = ThreadingProjection::etag(ModulePolicy::default());
    let root_id = "a".repeat(64);
    let reply_id = "b".repeat(64);

    projection.on_kernel_event(&event(&root_id, vec![]));
    projection.on_kernel_event(&event(&reply_id, vec![vec!["e", &root_id, "", "reply"]]));

    let snapshot = projection.snapshot();
    assert!(
        snapshot
            .blocks
            .iter()
            .any(|b| matches!(b, TimelineBlock::Module { events, .. } if events.len() == 2)),
        "expected a 2-event module block, got {:?}",
        snapshot.blocks
    );
}

#[test]
fn poisoned_state_degrades_to_empty_snapshot() {
    use std::panic::{self, AssertUnwindSafe};

    let projection = ThreadingProjection::etag(ModulePolicy::default());
    let poisoned = panic::catch_unwind(AssertUnwindSafe(|| {
        let _guard = projection.state.lock().unwrap();
        panic!("poison the mutex");
    }));
    assert!(poisoned.is_err());
    assert_eq!(projection.snapshot(), ThreadingSnapshot::empty());
}
