//! Integration tests for the `Grouper` algorithm. The grouper API is fully
//! public so unit-level coverage lives here rather than inline (keeps
//! `grouper.rs` under the AGENTS.md 500-LOC ceiling).
//!
//! The fake resolver below is intentionally distinct from the production
//! NIP-10 / NIP-22 resolvers — it isolates the algorithm from any
//! convention-specific tag decoding.

use nmp_core::substrate::KernelEvent;
use nmp_threading::{
    GroupDelta, Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock,
};

struct FakeResolver;

fn tag(key: &str, val: &str) -> Vec<String> {
    vec![key.into(), val.into()]
}

fn ev(id: &str, created_at: u64, parent: Option<&str>, root: Option<&str>) -> KernelEvent {
    let mut tags = Vec::new();
    if let Some(p) = parent {
        tags.push(tag("e_parent", p));
    }
    if let Some(r) = root {
        tags.push(tag("e_root", r));
    }
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at,
        tags,
        content: id.into(),
    }
}

fn ev_addr_root(id: &str, created_at: u64, parent: Option<&str>, coord: &str) -> KernelEvent {
    let mut tags = Vec::new();
    if let Some(p) = parent {
        tags.push(tag("e_parent", p));
    }
    tags.push(tag("a_root", coord));
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at,
        tags,
        content: id.into(),
    }
}

fn ev_uri_root(id: &str, created_at: u64, parent: Option<&str>, uri: &str) -> KernelEvent {
    let mut tags = Vec::new();
    if let Some(p) = parent {
        tags.push(tag("e_parent", p));
    }
    tags.push(tag("i_root", uri));
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at,
        tags,
        content: id.into(),
    }
}

impl ParentResolver for FakeResolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_parent" => Some(ThreadPointer::Event {
                id: v.clone(),
                relay: None,
                kind: None,
            }),
            _ => None,
        })
    }
    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_root" => Some(ThreadPointer::Event {
                id: v.clone(),
                relay: None,
                kind: None,
            }),
            (Some(k), Some(v)) if k == "a_root" => Some(ThreadPointer::Address {
                coord: v.clone(),
                relay: None,
                kind: None,
            }),
            (Some(k), Some(v)) if k == "i_root" => Some(ThreadPointer::External { uri: v.clone() }),
            _ => None,
        })
    }
    fn parent_author(&self, _event: &KernelEvent) -> Option<String> {
        None
    }
    fn supersedes(&self, event: &KernelEvent) -> Option<String> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_supersedes" => Some(v.clone()),
            _ => None,
        })
    }
}

fn ev_supersedes(id: &str, created_at: u64, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at,
        tags: vec![tag("e_supersedes", target)],
        content: id.into(),
    }
}

fn fresh() -> Grouper<FakeResolver> {
    Grouper::new(FakeResolver, ModulePolicy::default())
}

fn block_ids(b: &TimelineBlock) -> Vec<&str> {
    match b {
        TimelineBlock::Standalone { id, .. } => vec![id.as_str()],
        TimelineBlock::Module { events, .. } => events.iter().map(|s| s.as_str()).collect(),
    }
}

// ── Algorithm tests ─────────────────────────────────────────────────────

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
    // Fourth event must NOT join the same module — it spawns a new block.
    let _ = g.on_insert(&ev("D", 4, Some("C"), Some("A")));
    let module_count = g
        .blocks()
        .iter()
        .filter(|b| matches!(b, TimelineBlock::Module { .. }))
        .count();
    assert!(module_count >= 1);
    assert_eq!(g.blocks().len(), 2);
    // First (newest) block holds D, second block holds [A,B,C].
    let first_ids: Vec<&str> = block_ids(&g.blocks()[0]);
    let second_ids: Vec<&str> = block_ids(&g.blocks()[1]);
    assert!(first_ids.contains(&"D"));
    assert_eq!(second_ids, vec!["A", "B", "C"]);
}

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

#[test]
fn addressable_parent_terminates_walk() {
    let mut g = fresh();
    let comment = ev_addr_root("C", 1, None, "30023:alice:intro");
    assert!(matches!(
        g.on_insert(&comment),
        Some(GroupDelta::BlockInserted(0))
    ));
    assert_eq!(g.blocks().len(), 1);
    assert!(matches!(g.blocks()[0], TimelineBlock::Standalone { .. }));

    let reply = ev_addr_root("R", 2, Some("C"), "30023:alice:intro");
    let _ = g.on_insert(&reply);
    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Module { events, root, .. } => {
            assert_eq!(events, &vec!["C".to_string(), "R".to_string()]);
            assert!(matches!(root, Some(ThreadPointer::Address { .. })));
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn external_uri_root_drives_collapse() {
    let mut g = fresh();
    // Two separate chains anchored to the same external URI.
    let _ = g.on_insert(&ev_uri_root("P1", 1, None, "https://x.com/a"));
    let _ = g.on_insert(&ev_uri_root("R1", 2, Some("P1"), "https://x.com/a"));
    // Now there is a Module [P1, R1] with root = External.
    let pre_module_count = g
        .blocks()
        .iter()
        .filter(|b| matches!(b, TimelineBlock::Module { .. }))
        .count();
    assert_eq!(pre_module_count, 1);

    // Add a parallel chain — also two events, also same URI root.
    let _ = g.on_insert(&ev_uri_root("P2", 10, None, "https://x.com/a"));
    let _ = g.on_insert(&ev_uri_root("R2", 11, Some("P2"), "https://x.com/a"));

    // With default max_module_size=3 the merged length (4) doesn't fit so
    // collapse cannot fold both modules. The first (newest) Module exists
    // and carries the External root. The standalones may or may not be
    // present depending on splice path; what we pin down is that the
    // External-rooted Module persists.
    let modules_with_external_root: Vec<&TimelineBlock> = g
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b,
                TimelineBlock::Module {
                    root: Some(ThreadPointer::External { .. }),
                    ..
                }
            )
        })
        .collect();
    assert!(!modules_with_external_root.is_empty());
}

#[test]
fn external_uri_root_collapses_when_combined_fits() {
    // Two single-reply modules whose merged length is 4 — exceeds default
    // max_module_size=3. Bump the policy so the merge fires.
    let mut g = Grouper::new(
        FakeResolver,
        ModulePolicy {
            max_module_size: 6,
            ..ModulePolicy::default()
        },
    );
    let _ = g.on_insert(&ev_uri_root("P1", 1, None, "uri"));
    let _ = g.on_insert(&ev_uri_root("R1", 2, Some("P1"), "uri"));
    let _ = g.on_insert(&ev_uri_root("P2", 10, None, "uri"));
    let _ = g.on_insert(&ev_uri_root("R2", 11, Some("P2"), "uri"));

    let modules: Vec<&TimelineBlock> = g
        .blocks()
        .iter()
        .filter(|b| matches!(b, TimelineBlock::Module { .. }))
        .collect();
    // Collapse should fold the two Modules into one merged Module.
    assert_eq!(modules.len(), 1);
    if let TimelineBlock::Module { events, .. } = modules[0] {
        // Older chain first, then newer chain. Both pairs preserved.
        assert!(events.contains(&"P1".to_string()));
        assert!(events.contains(&"R1".to_string()));
        assert!(events.contains(&"P2".to_string()));
        assert!(events.contains(&"R2".to_string()));
    }
}

#[test]
fn collapse_disabled_keeps_modules_separate() {
    let mut g = Grouper::new(
        FakeResolver,
        ModulePolicy {
            max_module_size: 6,
            collapse_adjacent_same_root: false,
            ..ModulePolicy::default()
        },
    );
    let _ = g.on_insert(&ev_uri_root("A", 1, None, "uri"));
    let _ = g.on_insert(&ev_uri_root("B", 2, Some("A"), "uri"));
    let _ = g.on_insert(&ev_uri_root("C", 10, None, "uri"));
    let _ = g.on_insert(&ev_uri_root("D", 11, Some("C"), "uri"));
    let modules = g
        .blocks()
        .iter()
        .filter(|b| matches!(b, TimelineBlock::Module { .. }))
        .count();
    assert_eq!(modules, 2);
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

// ── Lossless Standalone root preservation (rung 2 regression guard) ──────
//
// A reply that cannot be stitched into a chain (parent absent / leaf
// taken / max_module_size hit) collapses to a length-1 chain. Before the
// rung-2 reshape that 1-event chain became `Standalone(id)`, DROPPING the
// resolved `terminal_root`, so a reply rendered as if it were a thread
// root. The reshape preserves the pointer so downstream renderers can flag
// it as a partial-chain head.

#[test]
fn length_one_reply_chain_preserves_root_pointer() {
    // S declares a root ("ROOT") but no in-store parent, so `walk_chain`
    // produces a single-element chain whose `terminal_root` is the declared
    // root hint. The emitted Standalone block must carry that root.
    let mut g = fresh();
    let delta = g.on_insert(&ev("S", 1, None, Some("ROOT")));
    assert!(matches!(delta, Some(GroupDelta::BlockInserted(0))));
    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Standalone { id, root } => {
            assert_eq!(id, "S");
            assert!(
                matches!(root, Some(ThreadPointer::Event { id, .. }) if id == "ROOT"),
                "length-1 reply chain must keep its resolved root pointer, got {root:?}"
            );
        }
        other => panic!("expected Standalone with root, got {other:?}"),
    }
}

#[test]
fn module_collapsed_to_standalone_on_removal_keeps_root() {
    // [ROOT_HINT-anchored] module [P, C] loses its mid event; the surviving
    // single event must remain a Standalone that still carries the module's
    // root pointer (the removal collapse path is the sibling of the
    // chain-build path and must not re-drop the root).
    let mut g = fresh();
    // P has a non-in-store root "ROOT" so the eventual module carries an
    // Event root pointer; C splices onto P.
    let _ = g.on_insert(&ev("P", 1, None, Some("ROOT")));
    let _ = g.on_insert(&ev("C", 2, Some("P"), Some("ROOT")));
    assert!(matches!(&g.blocks()[0], TimelineBlock::Module { .. }));

    // Remove the leaf; the module collapses to a single-event Standalone.
    let _ = g.on_remove(&"C".to_string());
    assert_eq!(g.blocks().len(), 1);
    match &g.blocks()[0] {
        TimelineBlock::Standalone { id, root } => {
            assert_eq!(id, "P");
            assert!(
                matches!(root, Some(ThreadPointer::Event { id, .. }) if id == "ROOT"),
                "collapsed Standalone must keep the module's root, got {root:?}"
            );
        }
        other => panic!("expected collapsed Standalone with root, got {other:?}"),
    }
}

// ── Supersession ────────────────────────────────────────────────────────
//
// Cross-NIP composition rule: a superseder evicts its target's standalone
// block from the layout, so e.g. a NIP-18 repost bumps the original note to
// the repost's position rather than duplicating it. Late-arriving targets
// are suppressed; removing all superseders restores the target.

#[test]
fn supersede_removes_standalone_target_already_in_layout() {
    let mut g = fresh();
    let _ = g.on_insert(&ev("R", 1, None, None));
    assert_eq!(g.blocks().len(), 1);

    let _ = g.on_insert(&ev_supersedes("S", 2, "R"));
    assert_eq!(g.blocks().len(), 1, "target's standalone block must be evicted");
    assert!(matches!(&g.blocks()[0], TimelineBlock::Standalone { id, .. } if id == "S"));
}

#[test]
fn supersede_suppresses_late_arriving_target() {
    let mut g = fresh();
    let _ = g.on_insert(&ev_supersedes("S", 2, "R"));
    assert_eq!(g.blocks().len(), 1);

    // Target arrives after its superseder — must not produce a duplicate block.
    let _ = g.on_insert(&ev("R", 1, None, None));
    assert_eq!(g.blocks().len(), 1, "late-arriving target must stay suppressed");
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

    assert_eq!(g.blocks().len(), 2, "expected the [R,C] module + the S block");
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
    assert_eq!(g.blocks().len(), 1, "R must come back once its superseder is gone");
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
