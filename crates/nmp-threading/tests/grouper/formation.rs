//! Insertion and module-formation behavior: a single event yields a
//! `Standalone` block, a reply splices onto its parent to promote it into a
//! `Module`, out-of-order ancestors stitch into a full chain once they
//! arrive, module size is capped per `ModulePolicy`, and re-inserting the
//! same id is a no-op.

use nmp_threading::{GroupDelta, TimelineBlock};

use super::support::{block_ids, ev, fresh};

#[test]
fn standalone_event_yields_one_block() {
    let mut g = fresh();
    let e = ev("A", 1, None, None);
    let delta = g.on_insert(&e);
    assert!(matches!(delta, Some(GroupDelta::BlockInserted(0))));
    assert_eq!(g.blocks().len(), 1);
    assert!(matches!(g.blocks()[0], TimelineBlock::Standalone { .. }));
    assert_eq!(block_ids(&g.blocks()[0]), vec!["A"]);
}

#[test]
fn two_message_merge_promotes_standalone_to_module() {
    let mut g = fresh();
    let parent = ev("P", 1, None, None);
    let reply = ev("R", 2, Some("P"), Some("P"));
    let _ = g.on_insert(&parent);
    let _ = g.on_insert(&reply);
    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Module {
            events, has_gap, ..
        } => {
            assert_eq!(events, &vec!["P".to_string(), "R".to_string()]);
            assert!(!has_gap);
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn reply_without_parent_buffers_until_arrival() {
    let mut g = fresh();
    let orphan = ev("R", 5, Some("P"), Some("P"));
    assert!(g.on_insert(&orphan).is_none());
    assert!(g.blocks().is_empty());
    assert!(g.pending_ancestor_ids().contains("P"));

    let parent = ev("P", 1, None, None);
    let _ = g.on_insert(&parent);
    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events, &vec!["P".to_string(), "R".to_string()]);
        }
        other => panic!("expected Module, got {other:?}"),
    }
    assert!(!g.pending_ancestor_ids().contains("P"));
}

#[test]
fn out_of_order_ancestor_arrival_stitches_full_chain() {
    let mut g = fresh();
    let grandchild = ev("G", 5, Some("C"), Some("P"));
    let child = ev("C", 3, Some("P"), Some("P"));
    let parent = ev("P", 1, None, None);

    assert!(g.on_insert(&grandchild).is_none());
    assert!(g.on_insert(&child).is_none());
    let _ = g.on_insert(&parent);

    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Module {
            events, has_gap, ..
        } => {
            assert_eq!(
                events,
                &vec!["P".to_string(), "C".to_string(), "G".to_string()]
            );
            assert!(!has_gap);
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn module_size_capped_at_policy_max() {
    let mut g = fresh(); // default max_module_size = 3
    let _ = g.on_insert(&ev("A", 1, None, None));
    let _ = g.on_insert(&ev("B", 2, Some("A"), Some("A")));
    let _ = g.on_insert(&ev("C", 3, Some("B"), Some("A")));
    let _ = g.on_insert(&ev("D", 4, Some("C"), Some("A")));
    let module_count = g
        .blocks()
        .iter()
        .filter(|b| matches!(b, TimelineBlock::Module { .. }))
        .count();
    assert!(module_count >= 1);
    assert_eq!(g.blocks().len(), 2);
    let first_ids: Vec<&str> = block_ids(&g.blocks()[0]);
    let second_ids: Vec<&str> = block_ids(&g.blocks()[1]);
    assert!(first_ids.contains(&"D"));
    assert_eq!(second_ids, vec!["A", "B", "C"]);
}

#[test]
fn dedup_same_id_never_appears_twice() {
    let mut g = fresh();
    let e = ev("X", 1, None, None);
    let _ = g.on_insert(&e);
    let _ = g.on_insert(&e);
    let _ = g.on_insert(&e);
    assert_eq!(g.blocks().len(), 1);

    let mut count = 0;
    for b in g.blocks() {
        for id in block_ids(b) {
            if id == "X" {
                count += 1;
            }
        }
    }
    assert_eq!(count, 1);
}
