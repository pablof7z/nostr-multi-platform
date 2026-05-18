//! `ViewModule` impls for the NIP-51 list family.
//!
//! Two views per the task brief:
//! - [`ListView`] — every list of a given `(author, kind)`, `created_at` desc.
//!   Its `Key` is the *true composite* `(PublicKey, u32)` (D8 — not a per-event
//!   alloc, not an `Option<author>`).
//! - [`ListDetailView`] — a single set resolved by `NaddrCoord`
//!   `(author, kind, d_tag)`, with the D1 `Placeholder<ListRecord>` +
//!   `source` contract and the cross-author / cross-kind coord-isolation guard.
//!
//! Both share the [`ListAccumulator`], which dedupes by `(author, kind, d_tag)`
//! per NIP-33 / replaceable (newest `created_at` wins). Decoding happens once
//! at insert (D8 hot-path discipline).

mod accumulator;
mod detail;
mod list;

pub use accumulator::{ListAccumulator, ListViewDelta};
pub use detail::{DetailState, ListDetailPayload, ListDetailSpec, ListDetailView};
pub use list::{ListListPayload, ListListSpec, ListView};

use nmp_core::substrate::ModuleRegistry;

/// Hex-encoded pubkey alias — surfaced here so view specs don't force callers
/// to import the planner module for one type alias (mirrors nip23).
pub type PublicKey = String;

/// Register the two ViewModules into a `ModuleRegistry`.
pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_view::<ListView>();
    registry.register_view::<ListDetailView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::{KIND_FOLLOW_SETS, KIND_MUTE_LIST};
    use nmp_core::planner::NaddrCoord;
    use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};

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

    #[test]
    fn list_view_key_is_composite_author_kind() {
        let k1 = ListView::key(&ListListSpec {
            author: "alice".into(),
            kind: KIND_FOLLOW_SETS,
        });
        let k2 = ListView::key(&ListListSpec {
            author: "alice".into(),
            kind: KIND_MUTE_LIST,
        });
        assert_ne!(k1, k2, "same author, different kind → distinct view key");
    }

    #[test]
    fn list_view_dependencies_pin_both_axes() {
        let deps = ListView::dependencies(&ListListSpec {
            author: "alice".into(),
            kind: KIND_FOLLOW_SETS,
        });
        assert_eq!(deps.kinds, vec![KIND_FOLLOW_SETS]);
        assert_eq!(deps.authors, vec!["alice".to_string()]);
    }

    #[test]
    fn list_view_dedup_keeps_newer_same_triple() {
        let (mut state, _) = ListView::open(
            &ViewContext::default(),
            ListListSpec {
                author: "alice".into(),
                kind: KIND_FOLLOW_SETS,
            },
        );
        let old = ke(
            "e1",
            KIND_FOLLOW_SETS,
            "alice",
            100,
            vec![vec!["d".into(), "f".into()]],
        );
        let new = ke(
            "e2",
            KIND_FOLLOW_SETS,
            "alice",
            200,
            vec![vec!["d".into(), "f".into()]],
        );
        ListView::on_event_inserted(&ViewContext::default(), &mut state, &old);
        ListView::on_event_inserted(&ViewContext::default(), &mut state, &new);
        let payload = ListView::snapshot(&ViewContext::default(), &state);
        assert_eq!(payload.lists.len(), 1);
        assert_eq!(payload.lists[0].event_id, "e2");
    }

    #[test]
    fn list_view_drops_non_nip51_events() {
        let (mut state, _) = ListView::open(
            &ViewContext::default(),
            ListListSpec {
                author: "alice".into(),
                kind: KIND_MUTE_LIST,
            },
        );
        let bad = ke("e1", 1, "alice", 100, vec![]);
        let delta = ListView::on_event_inserted(&ViewContext::default(), &mut state, &bad);
        assert!(delta.is_none());
        assert!(ListView::snapshot(&ViewContext::default(), &state)
            .lists
            .is_empty());
    }

    #[test]
    fn detail_view_dependencies_declare_full_triple() {
        let coord = NaddrCoord {
            pubkey: "alice".into(),
            kind: KIND_FOLLOW_SETS,
            d_tag: "friends".into(),
        };
        let deps = ListDetailView::dependencies(&ListDetailSpec { coord });
        assert_eq!(deps.kinds, vec![KIND_FOLLOW_SETS]);
        assert_eq!(deps.authors, vec!["alice".to_string()]);
        assert_eq!(
            deps.tag_refs,
            vec![("d".to_string(), "friends".to_string())]
        );
    }

    #[test]
    fn detail_view_placeholder_before_event() {
        let coord = NaddrCoord {
            pubkey: "alice".into(),
            kind: KIND_FOLLOW_SETS,
            d_tag: "friends".into(),
        };
        let (state, opened) = ListDetailView::open(
            &ViewContext::default(),
            ListDetailSpec {
                coord: coord.clone(),
            },
        );
        assert_eq!(opened.source, "placeholder");
        assert_eq!(opened.list.author, coord.pubkey);
        assert_eq!(opened.list.d_tag, coord.d_tag);
        assert!(opened.list.event_id.is_empty());
        let snap = ListDetailView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.source, "placeholder");
    }

    #[test]
    fn detail_view_rejects_same_d_different_kind() {
        // Cross-kind isolation: a mute list (kind 10000, d="") must not surface
        // when the view was opened for a follow set (kind 30000) — even though
        // the accumulator keys include kind, the coord filter is the first gate.
        let coord = NaddrCoord {
            pubkey: "alice".into(),
            kind: KIND_FOLLOW_SETS,
            d_tag: "friends".into(),
        };
        let (mut state, _) =
            ListDetailView::open(&ViewContext::default(), ListDetailSpec { coord });
        let wrong_kind = ke(
            "e1",
            KIND_MUTE_LIST,
            "alice",
            999,
            vec![vec!["d".into(), "friends".into()]],
        );
        let delta =
            ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &wrong_kind);
        assert!(delta.is_none(), "wrong-kind same-d event must be rejected");
        let snap = ListDetailView::snapshot(&ViewContext::default(), &state);
        assert_eq!(snap.source, "placeholder");
    }
}
