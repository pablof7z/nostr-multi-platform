use super::*;

const TARGET_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TARGET_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ALICE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const BOB: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const CAROL: &str = "3333333333333333333333333333333333333333333333333333333333333333";

/// Build a kind:7 reaction event targeting `target` (single `e` tag), with an
/// optional `["h", room]` group tag to mirror in-group reactions.
fn reaction(id: &str, author: &str, target: &str, content: &str, room: Option<&str>) -> KernelEvent {
    let mut tags = vec![vec!["e".to_string(), target.to_string()]];
    if let Some(room) = room {
        tags.push(vec!["h".to_string(), room.to_string()]);
    }
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_REACTION,
        created_at: 1,
        tags,
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn fresh_projection_is_empty() {
    let proj = ReactionAggregateProjection::new(None);
    assert_eq!(proj.snapshot(), ReactionAggregateSnapshot::empty());
    assert_eq!(proj.snapshot_json(), serde_json::json!({ "targets": [] }));
}

#[test]
fn counts_total_per_emoji_and_distinct_reactors() {
    let proj = ReactionAggregateProjection::new(None);
    proj.on_kernel_event(&reaction(&"1".repeat(64), ALICE, TARGET_A, "+", Some("room")));
    proj.on_kernel_event(&reaction(&"2".repeat(64), BOB, TARGET_A, "+", Some("room")));
    proj.on_kernel_event(&reaction(&"3".repeat(64), CAROL, TARGET_A, "🔥", Some("room")));

    let agg = proj.aggregate_for(TARGET_A).expect("target A present");
    assert_eq!(agg.total, 3);
    // "+" has 2, "🔥" has 1 → ordered by count desc.
    assert_eq!(
        agg.by_emoji,
        vec![
            ReactionEmojiCount { token: "+".into(), count: 2 },
            ReactionEmojiCount { token: "🔥".into(), count: 1 },
        ]
    );
    // Distinct reactor pubkeys, ascending.
    assert_eq!(agg.reactors, vec![ALICE.to_string(), BOB.to_string(), CAROL.to_string()]);
}

#[test]
fn empty_content_normalizes_to_plus_like() {
    let proj = ReactionAggregateProjection::new(None);
    proj.on_kernel_event(&reaction(&"1".repeat(64), ALICE, TARGET_A, "", None));
    proj.on_kernel_event(&reaction(&"2".repeat(64), BOB, TARGET_A, "   ", None));
    let agg = proj.aggregate_for(TARGET_A).unwrap();
    assert_eq!(agg.by_emoji, vec![ReactionEmojiCount { token: "+".into(), count: 2 }]);
}

#[test]
fn redelivery_is_idempotent() {
    let proj = ReactionAggregateProjection::new(None);
    let ev = reaction(&"1".repeat(64), ALICE, TARGET_A, "+", None);
    proj.on_kernel_event(&ev);
    proj.on_kernel_event(&ev);
    let agg = proj.aggregate_for(TARGET_A).unwrap();
    assert_eq!(agg.total, 1);
    assert_eq!(agg.reactors, vec![ALICE.to_string()]);
}

#[test]
fn targets_are_keyed_separately_and_sorted() {
    let proj = ReactionAggregateProjection::new(None);
    proj.on_kernel_event(&reaction(&"2".repeat(64), ALICE, TARGET_B, "+", None));
    proj.on_kernel_event(&reaction(&"1".repeat(64), BOB, TARGET_A, "+", None));
    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.targets.iter().map(|t| t.target_event_id.as_str()).collect();
    assert_eq!(ids, vec![TARGET_A, TARGET_B]);
}

#[test]
fn delete_by_original_reactor_removes_reaction() {
    let proj = ReactionAggregateProjection::new(None);
    let reaction_id = "1".repeat(64);
    proj.on_kernel_event(&reaction(&reaction_id, ALICE, TARGET_A, "+", None));

    // Alice retracts via a kind:5 deletion naming her reaction event.
    let delete = KernelEvent {
        id: "9".repeat(64),
        author: ALICE.to_string(),
        kind: KIND_REACTION_DELETE,
        created_at: 2,
        tags: vec![vec!["e".to_string(), reaction_id.clone()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&delete);
    assert!(proj.aggregate_for(TARGET_A).is_none());
}

#[test]
fn delete_by_other_pubkey_is_ignored() {
    let proj = ReactionAggregateProjection::new(None);
    let reaction_id = "1".repeat(64);
    proj.on_kernel_event(&reaction(&reaction_id, ALICE, TARGET_A, "+", None));

    let forged_delete = KernelEvent {
        id: "9".repeat(64),
        author: BOB.to_string(),
        kind: KIND_REACTION_DELETE,
        created_at: 2,
        tags: vec![vec!["e".to_string(), reaction_id]],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&forged_delete);
    assert_eq!(proj.aggregate_for(TARGET_A).unwrap().total, 1);
}

#[test]
fn last_e_tag_is_the_target() {
    // NIP-25: the reacted-to event is the LAST `e` tag (a thread-context `e`
    // tag may precede it).
    let proj = ReactionAggregateProjection::new(None);
    let ev = KernelEvent {
        id: "1".repeat(64),
        author: ALICE.to_string(),
        kind: KIND_REACTION,
        created_at: 1,
        tags: vec![
            vec!["e".to_string(), TARGET_B.to_string()], // thread root
            vec!["e".to_string(), TARGET_A.to_string()], // reacted-to event
        ],
        content: "+".to_string(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&ev);
    assert!(proj.aggregate_for(TARGET_A).is_some());
    assert!(proj.aggregate_for(TARGET_B).is_none());
}

#[test]
fn non_reaction_kinds_are_ignored() {
    let proj = ReactionAggregateProjection::new(None);
    let mut ev = reaction(&"1".repeat(64), ALICE, TARGET_A, "+", None);
    ev.kind = 9; // a chat message, not a reaction
    proj.on_kernel_event(&ev);
    assert_eq!(proj.snapshot(), ReactionAggregateSnapshot::empty());
}

#[test]
fn no_viewer_means_no_mine_handles() {
    let proj = ReactionAggregateProjection::new(None);
    proj.on_kernel_event(&reaction(&"1".repeat(64), ALICE, TARGET_A, "+", None));
    let agg = proj.aggregate_for(TARGET_A).unwrap();
    assert!(agg.mine.is_empty(), "no viewer → no retraction handles");
}

#[test]
fn mine_surfaces_only_the_viewers_own_reaction_ids() {
    // ALICE is the viewer. She reacted "+" (id a..) and "🔥" (id b..); BOB also
    // reacted "+". `mine` carries ONLY ALICE's two reactions, with the ids to
    // delete, ordered by token then id.
    let proj = ReactionAggregateProjection::new(Some(ALICE.to_string()));
    let alice_plus = "a".repeat(64);
    let alice_fire = "b".repeat(64);
    proj.on_kernel_event(&reaction(&alice_plus, ALICE, TARGET_A, "+", Some("room")));
    proj.on_kernel_event(&reaction(&alice_fire, ALICE, TARGET_A, "🔥", Some("room")));
    proj.on_kernel_event(&reaction(&"c".repeat(64), BOB, TARGET_A, "+", Some("room")));

    let agg = proj.aggregate_for(TARGET_A).unwrap();
    assert_eq!(agg.total, 3);
    assert_eq!(
        agg.mine,
        vec![
            ViewerReaction { token: "+".into(), reaction_event_id: alice_plus },
            ViewerReaction { token: "🔥".into(), reaction_event_id: alice_fire },
        ],
        "mine must carry the viewer's own kind:7 ids, no one else's"
    );
}

#[test]
fn relay_delivered_delete_decrements_and_clears_mine() {
    // The widened `kinds:[5,7]` interest delivers ALICE's own kind:5 deletion of
    // her kind:7. The aggregate decrements the count, drops her from reactors,
    // and clears her `mine` handle (toggle-off observed end-to-end).
    let proj = ReactionAggregateProjection::new(Some(ALICE.to_string()));
    let alice_reaction = "a".repeat(64);
    proj.on_kernel_event(&reaction(&alice_reaction, ALICE, TARGET_A, "+", Some("room")));
    proj.on_kernel_event(&reaction(&"c".repeat(64), BOB, TARGET_A, "+", Some("room")));
    assert_eq!(proj.aggregate_for(TARGET_A).unwrap().mine.len(), 1);

    let delete = KernelEvent {
        id: "9".repeat(64),
        author: ALICE.to_string(),
        kind: KIND_REACTION_DELETE,
        created_at: 3,
        tags: vec![vec!["e".to_string(), alice_reaction]],
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    proj.on_kernel_event(&delete);

    let agg = proj.aggregate_for(TARGET_A).unwrap();
    assert_eq!(agg.total, 1, "ALICE's reaction decremented");
    assert_eq!(agg.reactors, vec![BOB.to_string()]);
    assert!(agg.mine.is_empty(), "viewer's mine handle cleared after retract");
}

#[test]
fn set_viewer_pubkey_recomputes_mine() {
    let proj = ReactionAggregateProjection::new(None);
    proj.on_kernel_event(&reaction(&"a".repeat(64), ALICE, TARGET_A, "+", None));
    assert!(proj.aggregate_for(TARGET_A).unwrap().mine.is_empty());

    proj.set_viewer_pubkey(Some(ALICE.to_string()));
    assert_eq!(proj.aggregate_for(TARGET_A).unwrap().mine.len(), 1);

    proj.set_viewer_pubkey(None);
    assert!(proj.aggregate_for(TARGET_A).unwrap().mine.is_empty());
}
