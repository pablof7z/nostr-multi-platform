//! Lossless Standalone root preservation (rung 2 regression guard).
//!
//! A reply that cannot be stitched into a chain (parent absent / leaf
//! taken / max_module_size hit) collapses to a length-1 chain. Before the
//! rung-2 reshape that 1-event chain became `Standalone(id)`, DROPPING the
//! resolved `terminal_root`, so a reply rendered as if it were a thread
//! root. The reshape preserves the pointer so downstream renderers can flag
//! it as a partial-chain head.

use nmp_threading::{GroupDelta, ThreadPointer, TimelineBlock};

use super::support::{ev, fresh};

#[test]
fn standalone_wire_shape_is_object_with_optional_root() {
    // The serialized wire contract every non-Rust consumer depends on
    // (chirp-tui JSON fixtures + the iOS hand-decoder): `root: None` omits
    // the field; `root: Some(_)` nests the tagged ThreadPointer. Pins the
    // shape against future serde-attribute drift.
    let rootless = TimelineBlock::Standalone {
        id: "x".to_string(),
        root: None,
    };
    assert_eq!(
        serde_json::to_string(&rootless).expect("serialize rootless standalone"),
        r#"{"Standalone":{"id":"x"}}"#
    );

    let rooted = TimelineBlock::Standalone {
        id: "x".to_string(),
        root: Some(ThreadPointer::Event {
            id: "r".to_string(),
            relay: None,
            kind: None,
        }),
    };
    let json = serde_json::to_string(&rooted).expect("serialize rooted standalone");
    assert_eq!(
        json,
        r#"{"Standalone":{"id":"x","root":{"Event":{"id":"r"}}}}"#
    );

    // Round-trips back to the same value.
    let back: TimelineBlock = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, rooted);
}

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
