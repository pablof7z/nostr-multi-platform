use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_nip01::{
    ModularTimelineProjection, ModularTimelineSpec, NoteRelationCounts, RelationCount,
};
use nmp_threading::ModulePolicy;

fn spec() -> ModularTimelineSpec {
    ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: ModulePolicy::default(),
    }
}

fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    note_by(id, "auth", ts, tags)
}

fn note_by(id: &str, author: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind: 1,
        created_at: ts,
        tags,
        content: id.into(),
    }
}

fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
    note(
        id,
        ts,
        vec![
            vec!["e".into(), root.into(), "".into(), "root".into()],
            vec!["e".into(), parent.into(), "".into(), "reply".into()],
        ],
    )
}

fn profile_event(author: &str, display: &str) -> KernelEvent {
    KernelEvent {
        id: "profile".into(),
        author: author.into(),
        kind: 0,
        created_at: 3,
        tags: vec![],
        content: format!(r#"{{"display_name":"{display}"}}"#),
    }
}

#[test]
fn relation_counts_serialize_loading_vs_known_zero() {
    let counts = NoteRelationCounts {
        replies: RelationCount::known(0),
        reactions: RelationCount::known(0),
        reposts: RelationCount::known(0),
        zaps: RelationCount::known(0),
    };
    let json = serde_json::to_value(counts).expect("counts serialize");

    assert_eq!(json["replies"]["state"], "known");
    assert_eq!(json["replies"]["count"], 0);
    assert_eq!(json["reactions"]["state"], "known");
    assert_eq!(json["reposts"]["state"], "known");
    assert_eq!(json["zaps"]["state"], "known");
}

#[test]
fn cards_include_known_reply_counts_and_loading_relation_interests() {
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note("R", 1, vec![]));
    proj.on_kernel_event(&reply_to("C1", 2, "R", "R"));
    proj.on_kernel_event(&reply_to("C2", 3, "R", "R"));

    let snap = proj.snapshot();
    let root = snap.cards.iter().find(|c| c.id == "R").expect("root card");
    let child = snap
        .cards
        .iter()
        .find(|c| c.id == "C1")
        .expect("child card");

    assert_eq!(root.relation_counts.replies, RelationCount::known(2));
    assert_eq!(child.relation_counts.replies, RelationCount::known(0));
    assert_eq!(root.relation_counts.reactions, RelationCount::known(0));
    assert_eq!(root.relation_counts.reposts, RelationCount::known(0));
    assert_eq!(root.relation_counts.zaps, RelationCount::known(0));
}

#[test]
fn kind0_profile_refines_existing_author_display() {
    let author = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note_by("S", author, 1, vec![]));

    let before = proj.snapshot();
    let card = before.cards.iter().find(|c| c.id == "S").expect("card");
    assert_eq!(card.author_display.name, None);

    proj.on_kernel_event(&profile_event(author, "Alice"));

    let after = proj.snapshot();
    let card = after.cards.iter().find(|c| c.id == "S").expect("card");
    assert_eq!(card.author_display.name.as_deref(), Some("Alice"));
}

#[test]
fn reply_counts_handle_out_of_order_and_duplicate_delivery() {
    let proj = ModularTimelineProjection::new(&spec());
    let reply = reply_to("C", 2, "R", "R");
    proj.on_kernel_event(&reply);
    proj.on_kernel_event(&note("R", 1, vec![]));
    proj.on_kernel_event(&reply);

    let snap = proj.snapshot();
    let root = snap.cards.iter().find(|c| c.id == "R").expect("root card");

    assert_eq!(root.relation_counts.replies, RelationCount::known(1));
}

#[test]
fn relation_counts_include_reactions_reposts_and_zaps() {
    let target = "R";
    let proj = ModularTimelineProjection::new(&spec());
    proj.on_kernel_event(&note(target, 1, vec![]));
    proj.on_kernel_event(&KernelEvent {
        id: "react".into(),
        author: "alice".into(),
        kind: 7,
        created_at: 2,
        tags: vec![vec!["e".into(), target.into()]],
        content: "+".into(),
    });
    proj.on_kernel_event(&KernelEvent {
        id: "repost".into(),
        author: "bob".into(),
        kind: nmp_nip18::KIND_REPOST,
        created_at: 3,
        tags: vec![vec!["e".into(), target.into()]],
        content: String::new(),
    });
    proj.on_kernel_event(&KernelEvent {
        id: "zap".into(),
        author: "ln".into(),
        kind: nmp_nip57::KIND_ZAP_RECEIPT,
        created_at: 4,
        tags: vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), target.into()],
        ],
        content: String::new(),
    });

    let snap = proj.snapshot();
    let root = snap
        .cards
        .iter()
        .find(|c| c.id == target)
        .expect("root card");
    assert_eq!(root.relation_counts.reactions, RelationCount::known(1));
    assert_eq!(root.relation_counts.reposts, RelationCount::known(1));
    assert_eq!(root.relation_counts.zaps, RelationCount::known(1));
}
