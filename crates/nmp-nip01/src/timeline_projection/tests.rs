use super::*;
use nmp_threading::{ModulePolicy, TimelineBlock};
use std::sync::Arc;

fn spec() -> ModularTimelineSpec {
    ModularTimelineSpec {
        viewer: "me".into(),
        kinds: Vec::new(),
        authors: None,
        policy: ModulePolicy::default(),
    }
}

fn note(id: &str, author: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind: 1,
        created_at: ts,
        tags,
        content: id.into(),
        relay_provenance: Vec::new(),
    }
}

fn reply_to(id: &str, author: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    note(
        id,
        author,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
    )
}

#[test]
fn empty_open_yields_empty_snapshot() {
    let proj = ModularTimelineProjection::new(&spec());
    assert!(proj.snapshot().blocks.is_empty());
}

#[test]
fn root_plus_reply_collapses_into_one_module() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("R", "alice", 1, vec![]));
    proj.on_kernel_event(&reply_to("C", "bob", 2, "R", "R"));

    let snap = proj.snapshot();
    assert_eq!(snap.blocks.len(), 1);
    match &snap.blocks[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn standalone_event_becomes_standalone_block() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("S", "alice", 1, vec![]));
    let snap = proj.snapshot();
    assert_eq!(snap.blocks.len(), 1);
    assert!(matches!(snap.blocks[0], TimelineBlock::Standalone { .. }));
}

#[test]
fn blocks_sort_newest_first_by_indexed_events() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("old", "alice", 10, vec![]));
    proj.on_kernel_event(&note("new", "alice", 20, vec![]));

    let ids: Vec<_> = proj
        .snapshot()
        .blocks
        .into_iter()
        .map(|block| match block {
            TimelineBlock::Standalone { id, .. } => id,
            TimelineBlock::Module { events, .. } => events[0].clone(),
        })
        .collect();
    assert_eq!(ids, vec!["new", "old"]);
}

#[test]
fn suppression_hides_existing_blocks_by_author() {
    #[derive(Default)]
    struct SuppressAlice;
    impl SuppressionLookup for SuppressAlice {
        fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
            author_pubkey == "alice"
        }

        fn is_suppressed_event(&self, _event_id: &str) -> bool {
            false
        }
    }

    let mut proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("A", "alice", 1, vec![]));
    proj.on_kernel_event(&note("B", "bob", 2, vec![]));
    proj.set_suppression(Arc::new(SuppressAlice));

    let ids: Vec<_> = proj
        .snapshot()
        .blocks
        .into_iter()
        .map(|block| match block {
            TimelineBlock::Standalone { id, .. } => id,
            TimelineBlock::Module { events, .. } => events[0].clone(),
        })
        .collect();
    assert_eq!(ids, vec!["B"]);
}

#[test]
fn profile_events_are_ignored() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&KernelEvent {
        id: "profile".into(),
        author: "alice".into(),
        kind: 0,
        created_at: 1,
        tags: Vec::new(),
        content: "{}".into(),
        relay_provenance: Vec::new(),
    });
    assert!(proj.snapshot().blocks.is_empty());
}

#[test]
fn observed_projection_sink_ingests_notes() {
    let proj: Arc<dyn ObservedProjectionSink> = Arc::new(ModularTimelineProjection::new(&spec()));
    proj.on_kernel_event(&note("E", "alice", 1, vec![]));
}
