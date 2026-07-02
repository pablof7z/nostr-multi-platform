//! Block recency ordering: older backfilled events stay below newer blocks
//! rather than jumping to the top of the timeline, whether they arrive as a
//! lone event or as a whole thread.

use nmp_threading::GroupDelta;

use super::support::{block_ids, ev, fresh};

#[test]
fn older_backfill_stays_below_newer_blocks() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("NEW", 200, None, None));
    let delta = g.on_insert(&ev("OLD", 10, None, None));

    assert!(matches!(delta, Some(GroupDelta::BlockInserted(1))));
    assert_eq!(block_ids(&g.blocks()[0]), vec!["NEW"]);
    assert_eq!(block_ids(&g.blocks()[1]), vec!["OLD"]);
}

#[test]
fn older_thread_backfill_does_not_jump_to_top() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("NEW", 200, None, None));
    let _ = g.on_insert(&ev("OLD-P", 10, None, None));
    let _ = g.on_insert(&ev("OLD-R", 11, Some("OLD-P"), Some("OLD-P")));

    assert_eq!(block_ids(&g.blocks()[0]), vec!["NEW"]);
    assert_eq!(block_ids(&g.blocks()[1]), vec!["OLD-P", "OLD-R"]);
}
