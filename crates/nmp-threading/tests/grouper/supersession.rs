//! Cross-NIP composition rule: a superseder evicts its target's standalone
//! block from the layout, so e.g. a NIP-18 repost bumps the original note
//! to the repost's position rather than duplicating it. Late-arriving
//! targets are suppressed; removing all superseders restores the target.
//! A resolver that never overrides `supersedes` leaves the layout
//! untouched — that default no-op is pinned here too.

use nmp_core::substrate::KernelEvent;
use nmp_threading::{Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock};

use super::support::{ev, ev_supersedes, fresh};

#[test]
fn supersede_removes_standalone_target_already_in_layout() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("R", 1, None, None));
    assert_eq!(g.blocks().len(), 1);

    let _ = g.on_insert(&ev_supersedes("S", 2, "R"));
    assert_eq!(
        g.blocks().len(),
        1,
        "target's standalone block must be evicted"
    );
    assert!(matches!(&g.blocks()[0], TimelineBlock::Standalone { id, .. } if id == "S"));
}

#[test]
fn supersede_suppresses_late_arriving_target() {
    let mut g = fresh();
    let _ = g.on_insert(&ev_supersedes("S", 2, "R"));
    assert_eq!(g.blocks().len(), 1);

    // Target arrives after its superseder — must not produce a duplicate block.
    let _ = g.on_insert(&ev("R", 1, None, None));
    assert_eq!(
        g.blocks().len(),
        1,
        "late-arriving target must stay suppressed"
    );
    assert!(matches!(&g.blocks()[0], TimelineBlock::Standalone { id, .. } if id == "S"));
    // Target's payload is still recorded so chains can resolve it as a parent.
    assert!(g.event(&"R".to_string()).is_some());
}

#[test]
fn supersede_leaves_target_inside_a_module_chain_intact() {
    // R is the root of a reply chain [R, C]; even when superseded, R stays in
    // the chain so the reply still has parent context. Only standalone blocks
    // for R are evicted.
    let mut g = fresh();
    let _ = g.on_insert(&ev("R", 1, None, None));
    let _ = g.on_insert(&ev("C", 2, Some("R"), Some("R")));
    let _ = g.on_insert(&ev_supersedes("S", 3, "R"));

    assert_eq!(
        g.blocks().len(),
        2,
        "expected the [R,C] module + the S block"
    );
    let has_chain = g.blocks().iter().any(|b| {
        matches!(b, TimelineBlock::Module { events, .. } if events == &vec!["R".to_string(), "C".to_string()])
    });
    let has_superseder = g
        .blocks()
        .iter()
        .any(|b| matches!(b, TimelineBlock::Standalone { id, .. } if id == "S"));
    assert!(has_chain, "reply chain must survive");
    assert!(has_superseder, "superseder must be placed");
}

#[test]
fn removing_sole_superseder_restores_target_block() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("R", 1, None, None));
    let _ = g.on_insert(&ev_supersedes("S", 2, "R"));
    assert_eq!(g.blocks().len(), 1);

    let _ = g.on_remove(&"S".to_string());
    assert_eq!(
        g.blocks().len(),
        1,
        "R must come back once its superseder is gone"
    );
    assert!(matches!(&g.blocks()[0], TimelineBlock::Standalone { id, .. } if id == "R"));
}

#[test]
fn multiple_superseders_keep_target_suppressed_until_all_are_removed() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("R", 1, None, None));
    let _ = g.on_insert(&ev_supersedes("S1", 2, "R"));
    let _ = g.on_insert(&ev_supersedes("S2", 3, "R"));
    assert_eq!(g.blocks().len(), 2);

    let _ = g.on_remove(&"S1".to_string());
    let still_suppressed = !g
        .blocks()
        .iter()
        .any(|b| matches!(b, TimelineBlock::Standalone { id, .. } if id == "R"));
    assert!(still_suppressed, "R must stay suppressed while S2 remains");

    let _ = g.on_remove(&"S2".to_string());
    let restored = g
        .blocks()
        .iter()
        .any(|b| matches!(b, TimelineBlock::Standalone { id, .. } if id == "R"));
    assert!(restored, "R must be restored once every superseder is gone");
}

#[test]
fn default_resolver_supersedes_is_a_no_op() {
    // A resolver that doesn't override `supersedes` leaves the layout
    // untouched — two independent events render as two blocks.
    struct OnlyParents;
    impl ParentResolver for OnlyParents {
        fn parent(&self, _e: &KernelEvent) -> Option<ThreadPointer> {
            None
        }
        fn root(&self, _e: &KernelEvent) -> Option<ThreadPointer> {
            None
        }
        fn parent_author(&self, _e: &KernelEvent) -> Option<String> {
            None
        }
    }
    let mut g = Grouper::new(OnlyParents, ModulePolicy::default());
    let _ = g.on_insert(&ev("A", 1, None, None));
    let _ = g.on_insert(&ev("B", 2, None, None));
    assert_eq!(g.blocks().len(), 2);
}
