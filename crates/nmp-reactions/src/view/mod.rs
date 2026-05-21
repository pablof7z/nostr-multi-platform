//! Reactive views for NIP-25 reactions + NIP-18 reposts.
//!
//! Two views per the task brief:
//! - [`ReactionSummaryView`] — aggregate reactions for one target. Composite
//!   key = the [`crate::decode::ReactionTarget`]; per-`(reactor, target)`
//!   newest-wins collapse computed at snapshot.
//! - [`RepostsView`] — reposts (kinds 6/16) of a target, or by an author.
//!   Composite key = the [`RepostsSpec`] enum.
//!
//! Both share the [`ReactionAccumulator`], keyed on `event_id` for idempotency
//! (kinds 7/6/16 are regular events — not replaceable). Decode happens once at
//! insert (`try_from_kernel_event`, D8 hot path). Snapshots are deterministic
//! (count desc → content asc; records newest-first) for stable SwiftUI diffing.

mod accumulator;
mod reposts;
mod summary;

pub use accumulator::{ReactionAccumulator, ReactionViewDelta};
pub use reposts::{RepostsPayload, RepostsSpec, RepostsView};
pub use summary::{ReactionSummaryPayload, ReactionSummarySpec, ReactionSummaryView};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::ReactionTarget;
    use crate::kinds::{KIND_GENERIC_REPOST, KIND_REACTION, KIND_REPOST};
    use nmp_core::substrate::{KernelEvent, ViewContext};

    fn ke(
        id: &str,
        kind: u32,
        author: &str,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: author.into(),
            kind,
            created_at,
            tags,
            content: String::new(),
        }
    }

    fn reaction(id: &str, author: &str, ts: u64, target: &str, content: &str) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: author.into(),
            kind: KIND_REACTION,
            created_at: ts,
            tags: vec![vec!["e".into(), target.into()]],
            content: content.into(),
        }
    }

    #[test]
    fn summary_view_dependencies_declare_kind_7_and_e_tag() {
        let deps = ReactionSummaryView::dependencies(&ReactionSummarySpec {
            target: ReactionTarget::Event("evt1".into()),
        });
        assert_eq!(deps.kinds, vec![KIND_REACTION]);
        assert_eq!(deps.tag_refs, vec![("e".to_string(), "evt1".to_string())]);
    }

    #[test]
    fn summary_view_empty_payload_is_renderable_not_optional() {
        let (state, payload) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("evt1".into()),
            },
        );
        assert_eq!(payload.total, 0);
        assert!(payload.entries.is_empty());
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 0);
    }

    #[test]
    fn summary_view_aggregates_three_thumbs_one_heart() {
        let (mut state, _) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("X".into()),
            },
        );
        for (id, author) in [("r1", "a"), ("r2", "b"), ("r3", "c")] {
            ReactionSummaryView::on_event_inserted(
                &ViewContext::default(),
                &mut state,
                &reaction(id, author, 100, "X", "👍"),
            );
        }
        ReactionSummaryView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &reaction("r4", "d", 100, "X", "❤️"),
        );
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 4);
        let map: std::collections::HashMap<_, _> = snap.entries.into_iter().collect();
        assert_eq!(map.get("👍"), Some(&3));
        assert_eq!(map.get("❤️"), Some(&1));
    }

    #[test]
    fn summary_view_per_reactor_newest_wins() {
        // Same reactor 👍 (ts=100) then ❤️ (ts=200) on X → only ❤️:1.
        let (mut state, _) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("X".into()),
            },
        );
        ReactionSummaryView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &reaction("r1", "alice", 100, "X", "👍"),
        );
        ReactionSummaryView::on_event_inserted(
            &ViewContext::default(),
            &mut state,
            &reaction("r2", "alice", 200, "X", "❤️"),
        );
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 1, "one distinct reactor");
        assert_eq!(snap.entries, vec![("❤️".to_string(), 1)]);
    }

    #[test]
    fn summary_view_idempotent_on_duplicate_event_id() {
        let (mut state, _) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("X".into()),
            },
        );
        let evt = reaction("dup", "alice", 100, "X", "👍");
        let d1 = ReactionSummaryView::on_event_inserted(&ViewContext::default(), &mut state, &evt);
        let d2 = ReactionSummaryView::on_event_inserted(&ViewContext::default(), &mut state, &evt);
        assert!(d1.is_some());
        assert!(d2.is_none(), "second insert of same id is a no-op");
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 1, "count stays 1, not 2");
    }

    #[test]
    fn summary_view_cross_target_isolation() {
        // A reaction on Y must not appear in the summary for X.
        let (mut state, _) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("X".into()),
            },
        );
        let on_x = reaction("r1", "alice", 100, "X", "👍");
        let on_y = reaction("r2", "bob", 100, "Y", "👍");
        let dx = ReactionSummaryView::on_event_inserted(&ViewContext::default(), &mut state, &on_x);
        let dy = ReactionSummaryView::on_event_inserted(&ViewContext::default(), &mut state, &on_y);
        assert!(dx.is_some());
        assert!(dy.is_none(), "off-target reaction rejected");
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 1);
    }

    #[test]
    fn summary_view_drops_non_reaction_kinds() {
        let (mut state, _) = ReactionSummaryView::open(
            &ViewContext::default(),
            ReactionSummarySpec {
                target: ReactionTarget::Event("X".into()),
            },
        );
        let repost = ke(
            "r1",
            KIND_REPOST,
            "alice",
            100,
            vec![vec!["e".into(), "X".into()]],
        );
        let delta =
            ReactionSummaryView::on_event_inserted(&ViewContext::default(), &mut state, &repost);
        // The target matches X, so the accumulator stores it, but the summary
        // counts only kind:7 — so total stays 0.
        let _ = delta;
        let snap = ReactionSummaryView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.total, 0, "reposts excluded from reaction summary");
    }

    #[test]
    fn reposts_view_surfaces_kind_6_and_16() {
        let (mut state, _) = RepostsView::open(
            &ViewContext::default(),
            RepostsSpec::OfTarget(ReactionTarget::Event("X".into())),
        );
        let k6 = ke(
            "rp1",
            KIND_REPOST,
            "alice",
            100,
            vec![vec!["e".into(), "X".into()]],
        );
        let k16 = ke(
            "rp2",
            KIND_GENERIC_REPOST,
            "bob",
            200,
            vec![
                vec!["e".into(), "X".into()],
                vec!["k".into(), "30023".into()],
            ],
        );
        RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &k6);
        RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &k16);
        let snap = RepostsView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.reposts.len(), 2);
        // newest-first: rp2 (ts=200) before rp1 (ts=100).
        assert_eq!(snap.reposts[0].event_id, "rp2");
        match &snap.reposts[0].kind {
            crate::decode::ReactionKind::GenericRepost { original_kind, .. } => {
                assert_eq!(*original_kind, Some(30023));
            }
            _ => panic!("expected GenericRepost preserving original k"),
        }
    }

    #[test]
    fn reposts_view_excludes_reactions() {
        let (mut state, _) = RepostsView::open(
            &ViewContext::default(),
            RepostsSpec::OfTarget(ReactionTarget::Event("X".into())),
        );
        let react = reaction("r1", "alice", 100, "X", "+");
        let delta = RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &react);
        assert!(delta.is_none(), "kind:7 is not a repost");
        let snap = RepostsView::snapshot(&ViewContext::default(), &state);
        assert!(snap.reposts.is_empty());
    }

    #[test]
    fn reposts_view_by_author_scope() {
        let (mut state, _) = RepostsView::open(
            &ViewContext::default(),
            RepostsSpec::ByAuthor("alice".into()),
        );
        let alice_rp = ke(
            "rp1",
            KIND_REPOST,
            "alice",
            100,
            vec![vec!["e".into(), "X".into()]],
        );
        let bob_rp = ke(
            "rp2",
            KIND_REPOST,
            "bob",
            100,
            vec![vec!["e".into(), "Y".into()]],
        );
        RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &alice_rp);
        let bd = RepostsView::on_event_inserted(&ViewContext::default(), &mut state, &bob_rp);
        assert!(bd.is_none(), "bob's repost is off-author for an alice view");
        let snap = RepostsView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.reposts.len(), 1);
        assert_eq!(snap.reposts[0].author, "alice");
    }
}
