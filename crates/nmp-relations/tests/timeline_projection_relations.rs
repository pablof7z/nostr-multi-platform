//! Cross-protocol relation counting in `nmp_nip01::ModularTimelineProjection`,
//! driven by the `nmp-relations` `DefaultNoteRelationClassifier` injected via
//! `with_relation_classifier`. Moved out of `nmp-nip01` (#1728): asserting
//! reaction (NIP-25) / repost (NIP-18) / zap (NIP-57) counts requires those NIP
//! crates, which the base note crate no longer depends on.

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec, RelationCount};
use nmp_relations::default_note_relation_classifier;

fn spec() -> ModularTimelineSpec {
    ModularTimelineSpec {
        viewer: "me".into(),
        kinds: vec![],
        authors: None,
        policy: Default::default(),
    }
}

fn note(id: &str, ts: u64) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at: ts,
        tags: vec![],
        content: id.into(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn relation_counts_include_reactions_reposts_and_zaps() {
    let target = "R";
    let proj =
        ModularTimelineProjection::new(&spec()).with_relation_classifier(default_note_relation_classifier());
    proj.on_kernel_event(&note(target, 1));
    proj.on_kernel_event(&KernelEvent {
        id: "react".into(),
        author: "alice".into(),
        kind: 7,
        created_at: 2,
        tags: vec![vec!["e".into(), target.into()]],
        content: "+".into(),
        relay_provenance: Vec::new(),
    });
    proj.on_kernel_event(&KernelEvent {
        id: "repost".into(),
        author: "bob".into(),
        kind: nmp_nip18::KIND_REPOST,
        created_at: 3,
        tags: vec![vec!["e".into(), target.into()]],
        content: String::new(),
        relay_provenance: Vec::new(),
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
        relay_provenance: Vec::new(),
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
