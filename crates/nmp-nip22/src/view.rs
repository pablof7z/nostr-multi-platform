//! `CommentsView` — flat direct comments to a target event.
//!
//! Accepts a kind:1111 event whose decoded **lowercase** parent pointer (the
//! direct parent, not the root) is an `Event { id: spec.target, .. }`.
//! Address-targeted and external-URI-targeted comments don't show up in this
//! view shape — they'd need a `CommentsByAddressView` / `CommentsByUriView`
//! sibling, intentionally out of scope here.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use serde::{Deserialize, Serialize};

use crate::decode::{try_from_kernel_event, CommentPointer};
use crate::kinds::KIND_COMMENT;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommentsSpec {
    /// Target event id (the parent these comments reply to directly).
    pub target: EventId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CommentsPayload {
    pub target_id: EventId,
    pub comments: Vec<KernelEvent>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum CommentsDelta {
    Inserted(EventId),
    Removed(EventId),
    Replaced { old_id: EventId, new_id: EventId },
}

#[derive(Default)]
pub struct CommentsState {
    target: EventId,
    events: Vec<KernelEvent>,
}

impl CommentsState {
    fn accept(&self, event: &KernelEvent) -> bool {
        let Some(record) = try_from_kernel_event(event) else {
            return false;
        };
        matches!(&record.parent, CommentPointer::Event { id, .. } if id == &self.target)
    }

    fn insert(&mut self, event: &KernelEvent) -> Option<CommentsDelta> {
        if !self.accept(event) {
            return None;
        }
        if self.events.iter().any(|e| e.id == event.id) {
            return None;
        }
        self.events.push(event.clone());
        self.events.sort_by_key(|e| e.created_at);
        Some(CommentsDelta::Inserted(event.id.clone()))
    }

    fn remove(&mut self, id: &EventId) -> Option<CommentsDelta> {
        let before = self.events.len();
        self.events.retain(|e| e.id != *id);
        if self.events.len() == before {
            None
        } else {
            Some(CommentsDelta::Removed(id.clone()))
        }
    }

    fn replace(&mut self, old_id: &EventId, new_event: &KernelEvent) -> Option<CommentsDelta> {
        if !self.accept(new_event) {
            return self.remove(old_id);
        }
        let pos = self.events.iter().position(|e| e.id == *old_id)?;
        self.events[pos] = new_event.clone();
        self.events.sort_by_key(|e| e.created_at);
        Some(CommentsDelta::Replaced {
            old_id: old_id.clone(),
            new_id: new_event.id.clone(),
        })
    }
}

pub struct CommentsView;

impl CommentsView {
    pub const NAMESPACE: &'static str = "nmp.nip22.comments";

    pub fn key(spec: &CommentsSpec) -> EventId {
        spec.target.clone()
    }

    pub fn dependencies(spec: &CommentsSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_COMMENT],
            // NIP-22 parent event id sits in a lowercase `e` tag — the
            // subscription compiler routes by the tag-key/value pair, so the
            // hint is `("e", target)`.
            tag_refs: vec![("e".into(), spec.target.clone())],
            ..Default::default()
        }
    }

    pub fn open(_ctx: &ViewContext, spec: CommentsSpec) -> (CommentsState, CommentsPayload) {
        let state = CommentsState {
            target: spec.target.clone(),
            events: Vec::new(),
        };
        let payload = CommentsPayload {
            target_id: spec.target,
            comments: Vec::new(),
        };
        (state, payload)
    }

    pub fn on_event_inserted(
        _c: &ViewContext,
        s: &mut CommentsState,
        e: &KernelEvent,
    ) -> Option<CommentsDelta> {
        s.insert(e)
    }

    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut CommentsState,
        id: &EventId,
    ) -> Option<CommentsDelta> {
        s.remove(id)
    }

    pub fn on_event_replaced(
        _c: &ViewContext,
        s: &mut CommentsState,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<CommentsDelta> {
        s.replace(old, e)
    }

    pub fn snapshot(_c: &ViewContext, state: &CommentsState) -> CommentsPayload {
        CommentsPayload {
            target_id: state.target.clone(),
            comments: state.events.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ViewContext {
        ViewContext::default()
    }

    fn comment(id: &str, parent_id: &str, ts: u64) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1111,
            created_at: ts,
            tags: vec![
                vec!["E".into(), "ARTICLE".into()],
                vec!["K".into(), "30023".into()],
                vec!["e".into(), parent_id.into()],
                vec!["k".into(), "1111".into()],
            ],
            content: "x".into(),
        }
    }

    #[test]
    fn comments_view_filters_by_lowercase_parent() {
        let spec = CommentsSpec { target: "PARENT".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let c1 = comment("C1", "PARENT", 1);
        let c2 = comment("C2", "OTHER", 2);
        assert!(matches!(
            CommentsView::on_event_inserted(&ctx(), &mut state, &c1),
            Some(CommentsDelta::Inserted(_))
        ));
        assert!(CommentsView::on_event_inserted(&ctx(), &mut state, &c2).is_none());
        let snap = CommentsView::snapshot(&ctx(), &state);
        assert_eq!(snap.target_id, "PARENT");
        assert_eq!(snap.comments.len(), 1);
        assert_eq!(snap.comments[0].id, "C1");
    }

    #[test]
    fn comments_view_skips_h_tagged_kind_1111() {
        let spec = CommentsSpec { target: "PARENT".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let mut h_tagged = comment("CG", "PARENT", 1);
        h_tagged.tags.push(vec!["h".into(), "group".into()]);
        assert!(CommentsView::on_event_inserted(&ctx(), &mut state, &h_tagged).is_none());
    }

    #[test]
    fn comments_view_dedupes_and_sorts_by_created_at() {
        let spec = CommentsSpec { target: "PARENT".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let later = comment("L", "PARENT", 20);
        let earlier = comment("E", "PARENT", 10);
        CommentsView::on_event_inserted(&ctx(), &mut state, &later);
        CommentsView::on_event_inserted(&ctx(), &mut state, &earlier);
        assert!(CommentsView::on_event_inserted(&ctx(), &mut state, &later).is_none());
        let snap = CommentsView::snapshot(&ctx(), &state);
        let ids: Vec<&str> = snap.comments.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["E", "L"]);
    }

    #[test]
    fn comments_view_dependencies_lowercase_e_ref() {
        let spec = CommentsSpec { target: "TID".into() };
        let deps = CommentsView::dependencies(&spec);
        assert_eq!(deps.kinds, vec![KIND_COMMENT]);
        assert_eq!(deps.tag_refs, vec![("e".into(), "TID".into())]);
    }

    #[test]
    fn comments_view_admits_top_level_comment_when_target_is_root() {
        // A top-level comment carries only an uppercase `E` root and no
        // lowercase `e` parent. The decoder falls back parent == root, so the
        // view must surface it when the spec target equals that root id.
        let spec = CommentsSpec { target: "ARTICLE".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let top_level = KernelEvent {
            id: "TOP".into(),
            author: "auth".into(),
            kind: 1111,
            created_at: 1,
            tags: vec![
                vec!["E".into(), "ARTICLE".into()],
                vec!["K".into(), "30023".into()],
            ],
            content: "first!".into(),
        };
        assert!(matches!(
            CommentsView::on_event_inserted(&ctx(), &mut state, &top_level),
            Some(CommentsDelta::Inserted(_))
        ));
        let snap = CommentsView::snapshot(&ctx(), &state);
        assert_eq!(snap.comments.len(), 1);
        assert_eq!(snap.comments[0].id, "TOP");
    }

    #[test]
    fn comments_view_rejects_non_kind_1111() {
        // Only kind:1111 events are comments; a kind:1 note tagging the
        // target must not enter the projection.
        let spec = CommentsSpec { target: "PARENT".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let mut note = comment("N1", "PARENT", 1);
        note.kind = 1;
        assert!(CommentsView::on_event_inserted(&ctx(), &mut state, &note).is_none());
        assert!(CommentsView::snapshot(&ctx(), &state).comments.is_empty());
    }

    #[test]
    fn comments_view_replace_to_non_matching_event_removes_old() {
        // Replacing an in-view comment with an event whose parent no longer
        // points at the target evicts the stale entry.
        let spec = CommentsSpec { target: "PARENT".into() };
        let (mut state, _) = CommentsView::open(&ctx(), spec);
        let original = comment("C1", "PARENT", 1);
        CommentsView::on_event_inserted(&ctx(), &mut state, &original);
        let moved = comment("C1", "OTHER", 2);
        assert!(matches!(
            CommentsView::on_event_replaced(&ctx(), &mut state, &"C1".to_string(), &moved),
            Some(CommentsDelta::Removed(_))
        ));
        assert!(CommentsView::snapshot(&ctx(), &state).comments.is_empty());
    }
}
