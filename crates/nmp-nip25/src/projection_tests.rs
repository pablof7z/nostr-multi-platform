use super::*;

const TARGET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VIEWER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn event(id: &str, author: &str, kind: u32, created_at: u64, content: &str) -> KernelEvent {
    tagged_event(id, author, kind, created_at, content, TARGET)
}

fn tagged_event(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    content: &str,
    e_tag: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags: vec![vec!["e".to_string(), e_tag.to_string()]],
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn projection_indexes_reactions_by_target_and_viewer() {
    let projection = ReactionProjection::new(Some(VIEWER.to_string()));
    projection.on_kernel_event(&event(&"1".repeat(64), VIEWER, KIND_REACTION, 1, "+"));
    projection.on_kernel_event(&event(&"2".repeat(64), OTHER, KIND_REACTION, 2, "-"));

    let snapshot = projection.snapshot_for(TARGET);
    assert_eq!(snapshot.reactions.len(), 2);
    assert_eq!(
        snapshot.viewer_reaction,
        Some(ViewerReactionState {
            target_event_id: TARGET.to_string(),
            viewer_pubkey: VIEWER.to_string(),
            reaction_event_id: "1".repeat(64),
            content: "+".to_string(),
            created_at: 1,
        })
    );
}

#[test]
fn latest_viewer_reaction_wins() {
    let projection = ReactionProjection::new(None);
    projection.on_kernel_event(&event(&"1".repeat(64), VIEWER, KIND_REACTION, 1, "+"));
    projection.on_kernel_event(&event(&"2".repeat(64), VIEWER, KIND_REACTION, 2, "-"));

    let state = projection
        .viewer_reaction(TARGET, VIEWER)
        .expect("viewer reacted");
    assert_eq!(state.reaction_event_id, "2".repeat(64));
    assert_eq!(state.content, "-");
}

#[test]
fn kind5_delete_removes_own_reaction() {
    let projection = ReactionProjection::new(Some(VIEWER.to_string()));
    let reaction_id = "1".repeat(64);
    projection.on_kernel_event(&event(&reaction_id, VIEWER, KIND_REACTION, 1, "+"));
    projection.on_kernel_event(&tagged_event(
        &"9".repeat(64),
        VIEWER,
        KIND_REACTION_DELETE,
        2,
        "",
        &reaction_id,
    ));

    assert!(projection.viewer_reaction(TARGET, VIEWER).is_none());
    assert!(projection.snapshot_for(TARGET).reactions.is_empty());
}

#[test]
fn delete_from_other_author_does_not_remove_reaction() {
    let projection = ReactionProjection::new(None);
    let reaction_id = "1".repeat(64);
    projection.on_kernel_event(&event(&reaction_id, VIEWER, KIND_REACTION, 1, "+"));
    projection.on_kernel_event(&tagged_event(
        &"9".repeat(64),
        OTHER,
        KIND_REACTION_DELETE,
        2,
        "",
        &reaction_id,
    ));

    assert_eq!(projection.snapshot_for(TARGET).reactions.len(), 1);
}
