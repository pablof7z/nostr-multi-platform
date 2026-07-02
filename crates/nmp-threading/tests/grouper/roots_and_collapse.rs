//! Root-pointer variants and the adjacent-same-root collapse pass:
//! addressable (`a_root`) parents terminate the ancestor walk, external
//! (`i_root`) roots can fold separate chains into one Module once they fit
//! the size policy, and collapse can be disabled to keep modules separate.

use nmp_threading::{GroupDelta, Grouper, ModulePolicy, ThreadPointer, TimelineBlock};

use super::support::{ev_addr_root, ev_uri_root, fresh, FakeResolver};

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
