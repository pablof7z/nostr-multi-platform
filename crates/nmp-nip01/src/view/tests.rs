use super::*;

fn ke(
    id: &str,
    author: &str,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind: 1,
        created_at,
        tags,
        content: content.into(),
    }
}

fn ctx() -> ViewContext {
    ViewContext::default()
}

// ── RepliesView ────────────────────────────────────────────────────────

#[test]
fn replies_view_filters_by_reply_target() {
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);

    // Reply to ROOT — accepted.
    let r1 = ke(
        "R1",
        "alice",
        10,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "hi",
    );
    assert!(matches!(
        RepliesView::on_event_inserted(&ctx(), &mut state, &r1),
        Some(RepliesDelta::Inserted(_))
    ));

    // Reply to some other event — rejected.
    let r2 = ke(
        "R2",
        "bob",
        11,
        vec![vec!["e".into(), "OTHER".into(), "".into(), "reply".into()]],
        "no",
    );
    assert!(RepliesView::on_event_inserted(&ctx(), &mut state, &r2).is_none());

    let snapshot = RepliesView::snapshot(&ctx(), &state);
    assert_eq!(snapshot.target_id, "ROOT");
    assert_eq!(snapshot.replies.len(), 1);
    assert_eq!(snapshot.replies[0].id, "R1");
}

#[test]
fn replies_view_dedupes_and_sorts() {
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let r_later = ke(
        "LATER",
        "a",
        20,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "",
    );
    let r_earlier = ke(
        "EARLY",
        "a",
        10,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "",
    );

    let _ = RepliesView::on_event_inserted(&ctx(), &mut state, &r_later);
    let _ = RepliesView::on_event_inserted(&ctx(), &mut state, &r_earlier);
    // Duplicate insert returns None.
    assert!(RepliesView::on_event_inserted(&ctx(), &mut state, &r_later).is_none());

    let snap = RepliesView::snapshot(&ctx(), &state);
    let ids: Vec<&str> = snap.replies.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["EARLY", "LATER"]);
}

#[test]
fn replies_view_remove_clears_entry() {
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let r = ke(
        "R1",
        "a",
        1,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "",
    );
    RepliesView::on_event_inserted(&ctx(), &mut state, &r);
    let delta = RepliesView::on_event_removed(&ctx(), &mut state, &"R1".to_string());
    assert!(matches!(delta, Some(RepliesDelta::Removed(_))));
    assert!(RepliesView::snapshot(&ctx(), &state).replies.is_empty());
}

#[test]
fn replies_view_remove_unknown_id_is_a_noop() {
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    assert!(
        RepliesView::on_event_removed(&ctx(), &mut state, &"ghost".to_string()).is_none(),
        "removing an id that was never inserted yields no delta"
    );
}

#[test]
fn replies_view_rejects_a_thread_root_with_no_reply_marker() {
    // A kind-1 with no NIP-10 reply pointer is a thread root, not a reply
    // to `target` — it must be rejected even though it is kind 1.
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let root = ke("ROOT", "a", 1, vec![], "i am the root");
    assert!(RepliesView::on_event_inserted(&ctx(), &mut state, &root).is_none());
    assert!(RepliesView::snapshot(&ctx(), &state).replies.is_empty());
}

#[test]
fn replies_view_replace_with_matching_event_swaps_in_place() {
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let original = ke(
        "OLD",
        "a",
        5,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "v1",
    );
    RepliesView::on_event_inserted(&ctx(), &mut state, &original);

    let revised = ke(
        "NEW",
        "a",
        5,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "v2",
    );
    let delta = RepliesView::on_event_replaced(&ctx(), &mut state, &"OLD".to_string(), &revised);
    assert!(matches!(
        delta,
        Some(RepliesDelta::Replaced { old_id, new_id })
            if old_id == "OLD" && new_id == "NEW"
    ));
    let snap = RepliesView::snapshot(&ctx(), &state);
    assert_eq!(snap.replies.len(), 1);
    assert_eq!(snap.replies[0].id, "NEW");
    assert_eq!(snap.replies[0].content, "v2");
}

#[test]
fn replies_view_replace_with_non_matching_event_degrades_to_removal() {
    // If the replacement no longer points at `target`, the view drops the
    // old entry entirely rather than retaining a stale one.
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let original = ke(
        "OLD",
        "a",
        5,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "v1",
    );
    RepliesView::on_event_inserted(&ctx(), &mut state, &original);

    let moved_away = ke(
        "NEW",
        "a",
        5,
        vec![vec![
            "e".into(),
            "ELSEWHERE".into(),
            "".into(),
            "reply".into(),
        ]],
        "v2",
    );
    let delta = RepliesView::on_event_replaced(&ctx(), &mut state, &"OLD".to_string(), &moved_away);
    assert!(
        matches!(delta, Some(RepliesDelta::Removed(id)) if id == "OLD"),
        "a replacement that no longer matches the target removes the old entry"
    );
    assert!(RepliesView::snapshot(&ctx(), &state).replies.is_empty());
}

#[test]
fn replies_view_replace_of_unknown_id_with_matching_event_is_a_noop() {
    // The replacement matches the target, but the `old_id` was never in the
    // view → `position` returns None → no delta, nothing inserted.
    let spec = RepliesSpec {
        target: "ROOT".into(),
    };
    let (mut state, _) = RepliesView::open(&ctx(), spec);
    let revised = ke(
        "NEW",
        "a",
        5,
        vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]],
        "v2",
    );
    assert!(
        RepliesView::on_event_replaced(&ctx(), &mut state, &"ghost".to_string(), &revised)
            .is_none()
    );
    assert!(RepliesView::snapshot(&ctx(), &state).replies.is_empty());
}

// ── ThreadView ─────────────────────────────────────────────────────────

fn reply_marked(id: &str, author: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    ke(
        id,
        author,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
        "x",
    )
}

#[test]
fn thread_view_builds_tree_in_order() {
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let root = ke("R", "alice", 1, vec![], "root");
    let child1 = reply_marked("C1", "bob", 2, "R", "R");
    let child2 = reply_marked("C2", "carol", 3, "R", "R");
    let grandchild = reply_marked("G1", "dave", 4, "R", "C1");

    for ev in [&root, &child1, &child2, &grandchild] {
        ThreadView::on_event_inserted(&ctx(), &mut state, ev);
    }
    let snap = ThreadView::snapshot(&ctx(), &state);
    // DFS root-first: R, C1, G1, C2
    let ids: Vec<&str> = snap.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["R", "C1", "G1", "C2"]);
    let depths: Vec<u32> = snap.nodes.iter().map(|n| n.depth).collect();
    assert_eq!(depths, vec![0, 1, 2, 1]);
    // child_count on R == 2 (C1, C2); on C1 == 1 (G1)
    let r_node = &snap.nodes[0];
    assert_eq!(r_node.child_count, 2);
    let c1_node = &snap.nodes[1];
    assert_eq!(c1_node.child_count, 1);
}

#[test]
fn thread_view_handles_out_of_order_arrival() {
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);

    // Grandchild arrives before child.
    let grandchild = reply_marked("G1", "dave", 4, "R", "C1");
    let g_delta = ThreadView::on_event_inserted(&ctx(), &mut state, &grandchild);
    // No delta yet — parent C1 unknown.
    assert!(g_delta.is_none());

    // Now root.
    let root = ke("R", "alice", 1, vec![], "");
    ThreadView::on_event_inserted(&ctx(), &mut state, &root);

    // Now child arrives — should stitch grandchild.
    let child = reply_marked("C1", "bob", 2, "R", "R");
    ThreadView::on_event_inserted(&ctx(), &mut state, &child);

    let snap = ThreadView::snapshot(&ctx(), &state);
    let ids: Vec<&str> = snap.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["R", "C1", "G1"]);
}

#[test]
fn thread_view_dependencies_advertises_e_tag_ref() {
    let spec = ThreadSpec {
        root_event: "RID".into(),
    };
    let deps = ThreadView::dependencies(&spec);
    assert_eq!(deps.kinds, vec![KIND_SHORT_NOTE]);
    assert_eq!(deps.tag_refs, vec![("e".into(), "RID".into())]);
}

#[test]
fn thread_view_rejects_non_kind_1_events() {
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let mut not_a_note = ke("R", "alice", 1, vec![], "root");
    not_a_note.kind = 30023;
    assert!(ThreadView::on_event_inserted(&ctx(), &mut state, &not_a_note).is_none());
    assert!(ThreadView::snapshot(&ctx(), &state).nodes.is_empty());
}

#[test]
fn thread_view_ignores_a_reply_to_an_unrelated_thread() {
    // A kind-1 reply whose parent is neither the root nor anything known,
    // and which references no part of this thread, must not be buffered as
    // an orphan forever — but `insert` *does* buffer anything with a parent
    // pointer (it might join later). Assert it produces no visible node.
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let stray = reply_marked("S", "eve", 9, "OTHER_ROOT", "OTHER_PARENT");
    assert!(
        ThreadView::on_event_inserted(&ctx(), &mut state, &stray).is_none(),
        "a reply into an unknown subtree emits no delta until its parent shows up"
    );
    assert!(ThreadView::snapshot(&ctx(), &state).nodes.is_empty());
}

#[test]
fn thread_view_duplicate_insert_is_a_noop() {
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let root = ke("R", "alice", 1, vec![], "root");
    assert!(matches!(
        ThreadView::on_event_inserted(&ctx(), &mut state, &root),
        Some(ThreadDelta::NodeAdded(_))
    ));
    assert!(
        ThreadView::on_event_inserted(&ctx(), &mut state, &root).is_none(),
        "re-inserting the same id yields no second delta"
    );
    assert_eq!(ThreadView::snapshot(&ctx(), &state).nodes.len(), 1);
}

#[test]
fn thread_view_remove_root_drops_its_children_subtree() {
    // Removing a node cascades: its children lose their knowable parent and
    // are dropped from `by_id` too.
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let root = ke("R", "alice", 1, vec![], "root");
    let child = reply_marked("C1", "bob", 2, "R", "R");
    ThreadView::on_event_inserted(&ctx(), &mut state, &root);
    ThreadView::on_event_inserted(&ctx(), &mut state, &child);
    assert_eq!(ThreadView::snapshot(&ctx(), &state).nodes.len(), 2);

    let delta = ThreadView::on_event_removed(&ctx(), &mut state, &"R".to_string());
    assert!(matches!(delta, Some(ThreadDelta::NodeRemoved(id)) if id == "R"));
    assert!(
        ThreadView::snapshot(&ctx(), &state).nodes.is_empty(),
        "removing the root drops the dependent child too"
    );
}

#[test]
fn thread_view_remove_unknown_id_is_a_noop() {
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    assert!(ThreadView::on_event_removed(&ctx(), &mut state, &"ghost".to_string()).is_none());
}

#[test]
fn thread_view_replace_with_a_fresh_id_root_is_dropped() {
    // Replace is remove(old) + insert(new). The new event carries a fresh
    // id that is neither the spec root nor a reply to anything known, so
    // insert() finds no parent and drops it — no delta, empty tree.
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let root = ke("R", "alice", 1, vec![], "original root");
    ThreadView::on_event_inserted(&ctx(), &mut state, &root);

    let revised = ke("R2", "alice", 1, vec![], "edited root");
    // The replacement's id is not the spec root and it has no parent
    // pointer, so it is dropped rather than re-rooted. Assert that contract.
    let delta = ThreadView::on_event_replaced(&ctx(), &mut state, &"R".to_string(), &revised);
    assert!(
        delta.is_none(),
        "a replacement whose id is not the spec root and has no parent is dropped"
    );
    assert!(ThreadView::snapshot(&ctx(), &state).nodes.is_empty());
}

#[test]
fn thread_view_replace_same_id_root_keeps_it_visible() {
    // A genuine replaceable-event style replace that reuses the root id
    // must keep the root in the tree with the new content.
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    let root = ke("R", "alice", 1, vec![], "v1");
    ThreadView::on_event_inserted(&ctx(), &mut state, &root);

    let same_id_revised = ke("R", "alice", 1, vec![], "v2");
    let delta =
        ThreadView::on_event_replaced(&ctx(), &mut state, &"R".to_string(), &same_id_revised);
    assert!(matches!(delta, Some(ThreadDelta::NodeAdded(id)) if id == "R"));
    let snap = ThreadView::snapshot(&ctx(), &state);
    assert_eq!(snap.nodes.len(), 1);
    assert_eq!(snap.nodes[0].content, "v2");
}

#[test]
fn thread_view_forest_mode_when_root_absent() {
    // The root has not arrived, but a direct child of the root has. The
    // flatten() forest branch emits that subtree rooted at depth 0.
    let spec = ThreadSpec {
        root_event: "R".into(),
    };
    let (mut state, _) = ThreadView::open(&ctx(), spec);
    // C1 replies directly to root id R; R itself never arrives.
    let child = reply_marked("C1", "bob", 2, "R", "R");
    let delta = ThreadView::on_event_inserted(&ctx(), &mut state, &child);
    assert!(
        matches!(delta, Some(ThreadDelta::NodeAdded(id)) if id == "C1"),
        "a direct reply to the root id is in-thread even before the root arrives"
    );
    let snap = ThreadView::snapshot(&ctx(), &state);
    assert_eq!(snap.nodes.len(), 1);
    assert_eq!(snap.nodes[0].id, "C1");
    assert_eq!(
        snap.nodes[0].depth, 0,
        "forest subtree root sits at depth 0"
    );
    assert_eq!(snap.nodes[0].parent_id, None);
}
