//! Post-insert mutation behavior: removing an event drops a standalone
//! block or opens a gap mid-module, replace swaps an id in place, and
//! `has_gap` is raised both by a lookback-threshold time jump and by a
//! declared root id that doesn't match the resolved chain.

use nmp_threading::{GroupDelta, TimelineBlock};

use super::support::{block_ids, ev, fresh};

#[test]
fn on_remove_drops_standalone_block() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("A", 1, None, None));
    let d = g.on_remove(&"A".to_string());
    assert!(matches!(d, Some(GroupDelta::BlockRemoved(0))));
    assert!(g.blocks().is_empty());
}

#[test]
fn on_remove_mid_module_introduces_gap() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("A", 1, None, None));
    let _ = g.on_insert(&ev("B", 2, Some("A"), Some("A")));
    let _ = g.on_insert(&ev("C", 3, Some("B"), Some("A")));
    let _ = g.on_remove(&"B".to_string());
    match &g.blocks()[0] {
        TimelineBlock::Module {
            events, has_gap, ..
        } => {
            assert_eq!(events, &vec!["A".to_string(), "C".to_string()]);
            assert!(*has_gap);
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn on_replace_swaps_event_in_chain() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("A", 1, None, None));
    let _ = g.on_insert(&ev("B", 2, Some("A"), Some("A")));
    // Replace A with a new event (different id).
    let new_a = ev("A2", 5, None, None);
    let _ = g.on_replace(&"A".to_string(), &new_a);
    let any_a2 = g.blocks().iter().any(|b| block_ids(b).contains(&"A2"));
    assert!(any_a2);
    let any_a_original = g.blocks().iter().any(|b| block_ids(b).contains(&"A"));
    assert!(!any_a_original);
}

#[test]
fn lookback_gap_marks_has_gap() {
    let mut g = fresh(); // 72h threshold
    let _ = g.on_insert(&ev("A", 1, None, None));
    let way_later = 1 + 72 * 3600 + 100;
    let _ = g.on_insert(&ev("B", way_later, Some("A"), Some("A")));
    match &g.blocks()[0] {
        TimelineBlock::Module { has_gap, .. } => assert!(*has_gap),
        _ => panic!("expected Module"),
    }
}

#[test]
fn mismatched_root_id_marks_has_gap() {
    // Reply declares a root id that doesn't match the chain top.
    let mut g = fresh();
    let _ = g.on_insert(&ev("MID", 1, None, None));
    // R's parent is MID (in store), root is "ROOT" (not in store, not in chain).
    let _ = g.on_insert(&ev("R", 2, Some("MID"), Some("ROOT")));
    match &g.blocks()[0] {
        TimelineBlock::Module { has_gap, .. } => assert!(*has_gap),
        TimelineBlock::Standalone { .. } => {
            // The reply may have splicd onto MID and adopted the
            // mismatched-root hint; the resulting Module should have
            // has_gap = true. Reach the module via the splice path test.
            panic!("expected Module after splice");
        }
    }
}
