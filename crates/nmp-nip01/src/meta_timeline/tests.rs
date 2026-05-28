use super::*;

fn ctx() -> ViewContext {
    ViewContext::default()
}

fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at: ts,
        tags,
        content: id.into(),
    }
}

fn repost(id: &str, ts: u64, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "reposter".into(),
        kind: KIND_REPOST,
        created_at: ts,
        tags: vec![vec!["e".into(), target.into()]],
        content: String::new(),
    }
}

fn marked(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    note(
        id,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
    )
}

#[test]
fn empty_open_yields_empty_payload() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let (_state, payload) = Nip10ModularTimelineView::open(&ctx(), &spec);
    assert!(payload.blocks.is_empty());
}

#[test]
fn dependencies_default_to_kind_1() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let deps = Nip10ModularTimelineView::dependencies(&spec);
    assert_eq!(deps.kinds, vec![1]);
    assert!(deps.authors.is_empty());
}

#[test]
fn root_plus_reply_form_single_module() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), &spec);
    let root = note("R", 1, vec![]);
    let reply = marked("C", 2, "R", "R");
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &root);
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &reply);
    let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);
    assert_eq!(snap.blocks.len(), 1);
    match &snap.blocks[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
        }
        other => panic!("expected Module, got {other:?}"),
    }
}

#[test]
fn rejects_wrong_kind() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), &spec);
    let mut e = note("Z", 1, vec![]);
    e.kind = 30023;
    assert!(Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &e).is_none());
    assert!(Nip10ModularTimelineView::snapshot(&ctx(), &s)
        .blocks
        .is_empty());
}

#[test]
fn author_filter_excludes_others() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: Some(vec!["alice".into()]),
        policy: ModulePolicy::default(),
    };
    let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), &spec);
    let mut a = note("A", 1, vec![]);
    a.author = "alice".into();
    let mut b = note("B", 2, vec![]);
    b.author = "bob".into();
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &a);
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &b);
    let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);
    assert_eq!(snap.blocks.len(), 1);
}

#[test]
fn repost_supersedes_original_and_keeps_layout_to_one_block() {
    // A kind:6 repost of a note already in the feed bumps the original to the
    // repost's position rather than stacking a second block — NIP-18
    // supersession via `ParentResolver::supersedes`.
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![KIND_SHORT_NOTE, KIND_REPOST],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), &spec);
    let root = note("R", 1, vec![]);
    let boost = repost("B", 2, "R");

    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &root);
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &boost);
    let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);

    assert_eq!(
        snap.blocks.len(),
        1,
        "repost must evict the original's block"
    );
    assert!(matches!(
        &snap.blocks[0],
        TimelineBlock::Standalone { id, .. } if id == "B"
    ));
}

#[test]
fn repost_arriving_before_original_suppresses_the_late_original() {
    // Relay-order opposite: the repost reaches us first, then the kind:1 it
    // targets. The original must still be suppressed so the note renders once
    // at the repost's slot.
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![KIND_SHORT_NOTE, KIND_REPOST],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), &spec);
    let boost = repost("B", 2, "R");
    let root = note("R", 1, vec![]);

    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &boost);
    let _ = Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &root);
    let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);

    assert_eq!(
        snap.blocks.len(),
        1,
        "late-arriving original must stay suppressed"
    );
    assert!(matches!(
        &snap.blocks[0],
        TimelineBlock::Standalone { id, .. } if id == "B"
    ));
}

#[test]
fn nip10_resolver_supersedes_returns_target_only_for_kind_6() {
    let plain = note("X", 1, vec![]);
    assert!(Nip10Resolver.supersedes(&plain).is_none());

    let boost = repost("B", 2, "R");
    assert_eq!(Nip10Resolver.supersedes(&boost).as_deref(), Some("R"));
}

#[test]
fn effective_kinds_defaults_to_kind_1_when_empty() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    };
    assert_eq!(spec.effective_kinds(), vec![KIND_SHORT_NOTE]);
}

#[test]
fn effective_kinds_passes_explicit_kinds_through_verbatim() {
    let spec = ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![1, 6, 16],
        authors: None,
        policy: ModulePolicy::default(),
    };
    // No defaulting, no sorting at this layer — `key()` owns ordering.
    assert_eq!(spec.effective_kinds(), vec![1, 6, 16]);
}

fn spec_with(kinds: Vec<u32>, authors: Option<Vec<&str>>) -> ModularTimelineSpec {
    ModularTimelineSpec {
        viewer: "viewer".into(),
        kinds,
        authors: authors.map(|v| v.into_iter().map(String::from).collect()),
        policy: ModulePolicy::default(),
    }
}

#[test]
fn key_is_order_independent_for_kinds() {
    // Two specs with the same kind *set* in different input order must key
    // identically — otherwise the same logical view opens twice.
    let a = Nip10ModularTimelineView::key(&spec_with(vec![16, 1, 6], None));
    let b = Nip10ModularTimelineView::key(&spec_with(vec![1, 6, 16], None));
    assert_eq!(a, b);
}

#[test]
fn key_is_order_independent_for_authors() {
    let a = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["carol", "alice", "bob"])));
    let b = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["alice", "bob", "carol"])));
    assert_eq!(a, b);
}

#[test]
fn key_distinguishes_different_viewers_and_author_sets() {
    let one = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["alice"])));
    let two = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["bob"])));
    assert_ne!(one, two, "different author sets must not collide");

    // `None` authors and `Some(empty)` authors are distinguishable from a
    // populated set.
    let no_filter = Nip10ModularTimelineView::key(&spec_with(vec![], None));
    assert_ne!(no_filter, one);
}

#[test]
fn key_empty_kinds_resolves_to_the_kind_1_default() {
    // `key()` uses `effective_kinds()`, so an empty-kinds spec and an
    // explicit `[1]` spec must produce the same key.
    let defaulted = Nip10ModularTimelineView::key(&spec_with(vec![], None));
    let explicit = Nip10ModularTimelineView::key(&spec_with(vec![1], None));
    assert_eq!(defaulted, explicit);
}

#[test]
fn group_delta_converts_to_modular_timeline_delta_for_every_arm() {
    assert_eq!(
        ModularTimelineDelta::from(GroupDelta::BlockInserted(3)),
        ModularTimelineDelta::BlockInserted(3)
    );
    assert_eq!(
        ModularTimelineDelta::from(GroupDelta::BlockReplaced(7)),
        ModularTimelineDelta::BlockReplaced(7)
    );
    assert_eq!(
        ModularTimelineDelta::from(GroupDelta::BlockRemoved(0)),
        ModularTimelineDelta::BlockRemoved(0)
    );
}

#[test]
fn resolver_extracts_reply_and_root_pointers() {
    let reply = marked("C", 2, "ROOT", "PARENT");
    match Nip10Resolver.parent(&reply) {
        Some(ThreadPointer::Event { id, kind, .. }) => {
            assert_eq!(id, "PARENT");
            assert_eq!(kind, None);
        }
        other => panic!("expected an Event parent pointer, got {other:?}"),
    }
    match Nip10Resolver.root(&reply) {
        Some(ThreadPointer::Event { id, .. }) => assert_eq!(id, "ROOT"),
        other => panic!("expected an Event root pointer, got {other:?}"),
    }
}

#[test]
fn resolver_returns_none_for_a_thread_root() {
    // A root note carries no NIP-10 markers — both parent() and root()
    // resolve to None so the grouper treats it as a module head.
    let root = note("R", 1, vec![]);
    assert!(Nip10Resolver.parent(&root).is_none());
    assert!(Nip10Resolver.root(&root).is_none());
    assert!(Nip10Resolver.parent_author(&root).is_none());
}

#[test]
fn resolver_ignores_repost_e_tags_as_thread_edges() {
    let boost = repost("B", 2, "R");

    assert!(Nip10Resolver.parent(&boost).is_none());
    assert!(Nip10Resolver.root(&boost).is_none());
    assert!(Nip10Resolver.parent_author(&boost).is_none());
}

#[test]
fn resolver_parent_author_returns_first_p_tag_as_a_hint() {
    // NIP-10 gives no positional guarantee for p-tags; the resolver
    // surfaces the first p-tag as a best-effort hint. Pin that behaviour.
    let mut e = marked("C", 2, "ROOT", "PARENT");
    e.tags.push(vec!["p".into(), "first-pubkey".into()]);
    e.tags.push(vec!["p".into(), "second-pubkey".into()]);
    assert_eq!(
        Nip10Resolver.parent_author(&e),
        Some("first-pubkey".to_string())
    );
}

#[test]
fn resolver_carries_relay_hint_from_marked_e_tag() {
    // A marked `e` tag with a relay column must surface that relay on the
    // resolved ThreadPointer.
    let e = note(
        "C",
        2,
        vec![vec![
            "e".into(),
            "PARENT".into(),
            "wss://relay.example".into(),
            "reply".into(),
        ]],
    );
    match Nip10Resolver.parent(&e) {
        Some(ThreadPointer::Event { id, relay, .. }) => {
            assert_eq!(id, "PARENT");
            assert_eq!(relay.as_deref(), Some("wss://relay.example"));
        }
        other => panic!("expected an Event pointer with relay, got {other:?}"),
    }
}
