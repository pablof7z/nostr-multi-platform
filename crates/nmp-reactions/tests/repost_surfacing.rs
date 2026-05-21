//! Repost surfacing: kind:6 and kind:16 of event X surfaced by `RepostsView`;
//! a generic repost preserves its original `k`.

use nmp_core::substrate::{KernelEvent, ViewContext};
use nmp_relations::{
    ReactionTarget, RepostsSpec, RepostsView, ReactionKind, KIND_GENERIC_REPOST, KIND_REPOST,
};

const X: &str = "event-X-0000000000000000000000000000000000000000000000000000000000";

fn ke(id: &str, kind: u32, author: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind,
        created_at: ts,
        tags,
        content: String::new(),
    }
}

#[test]
fn reposts_view_surfaces_kind_6_and_16_of_target() {
    let (mut state, payload) = RepostsView::open(
        &ViewContext::default(),
        RepostsSpec::OfTarget(ReactionTarget::Event(X.to_string())),
    );
    assert!(payload.reposts.is_empty(), "D1: empty is renderable");

    let k6 = ke(
        "rp6",
        KIND_REPOST,
        "alice",
        100,
        vec![vec!["e".into(), X.into()]],
    );
    let k16 = ke(
        "rp16",
        KIND_GENERIC_REPOST,
        "bob",
        200,
        vec![vec!["e".into(), X.into()], vec!["k".into(), "30023".into()]],
    );
    RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &k6);
    RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &k16);

    let snap = RepostsView::snapshot(&ViewContext::default(), &state);
    assert_eq!(snap.reposts.len(), 2);
    // newest-first.
    assert_eq!(snap.reposts[0].event_id, "rp16");
    assert_eq!(snap.reposts[1].event_id, "rp6");

    // Generic repost preserves the original kind.
    match &snap.reposts[0].kind {
        ReactionKind::GenericRepost { original_kind, .. } => {
            assert_eq!(*original_kind, Some(30023));
        }
        other => panic!("expected GenericRepost, got {other:?}"),
    }
    assert!(matches!(snap.reposts[1].kind, ReactionKind::Repost { .. }));
}

#[test]
fn reposts_view_rejects_off_target_repost() {
    let (mut state, _) = RepostsView::open(
        &ViewContext::default(),
        RepostsSpec::OfTarget(ReactionTarget::Event(X.to_string())),
    );
    let off = ke(
        "rpY",
        KIND_REPOST,
        "alice",
        100,
        vec![vec!["e".into(), "other-event".into()]],
    );
    let delta = RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &off);
    assert!(delta.is_none(), "off-target repost rejected");
    let snap = RepostsView::snapshot(&ViewContext::default(), &state);
    assert!(snap.reposts.is_empty());
}
