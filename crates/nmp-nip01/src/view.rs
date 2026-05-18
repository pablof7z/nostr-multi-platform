//! `RepliesView` (flat direct replies) and `ThreadView` (parent/child tree).
//!
//! Both `ViewModule` impls accept kind-1 events whose [`Nip10Refs`] chain
//! them into the target / root the spec names. The planner is responsible
//! for surfacing the kind-1 stream — `ViewDependencies` declares
//! `kinds: vec![1]` plus a single `("e", target)` tag-ref hint so the
//! subscription compiler can route efficiently.
//!
//! ## Lazy `#e` expansion (ThreadView)
//!
//! `view-catalog.md §5` calls for replies-of-replies to expand the `#e` set
//! lazily as nested replies arrive. The current [`ViewModule::dependencies`]
//! is a static snapshot — there is no API to mutate dependencies post-open.
//! This crate therefore relies on the planner also surfacing nested replies
//! (e.g. via a separate `RepliesView` per intermediate node). When a child
//! reply that points at an as-yet-unseen parent arrives, it is buffered in
//! the `orphans` table and stitched once the parent does arrive — matching
//! applesauce's `ThreadModel.parentReferences` behaviour.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;
use crate::kinds::KIND_SHORT_NOTE;

// ─── RepliesView ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepliesSpec {
    pub target: EventId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepliesPayload {
    pub target_id: EventId,
    pub replies: Vec<KernelEvent>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum RepliesDelta {
    Inserted(EventId),
    Removed(EventId),
    Replaced { old_id: EventId, new_id: EventId },
}

#[derive(Default)]
pub struct RepliesState {
    target: EventId,
    events: Vec<KernelEvent>,
}

impl RepliesState {
    fn accept(&self, event: &KernelEvent) -> bool {
        let Some(record) = try_from_kernel_event(event) else {
            return false;
        };
        record
            .refs
            .reply
            .as_ref()
            .map(|r| r.id == self.target)
            .unwrap_or(false)
    }

    fn insert(&mut self, event: &KernelEvent) -> Option<RepliesDelta> {
        if !self.accept(event) {
            return None;
        }
        if self.events.iter().any(|e| e.id == event.id) {
            return None;
        }
        self.events.push(event.clone());
        self.events.sort_by_key(|e| e.created_at);
        Some(RepliesDelta::Inserted(event.id.clone()))
    }

    fn remove(&mut self, id: &EventId) -> Option<RepliesDelta> {
        let before = self.events.len();
        self.events.retain(|e| e.id != *id);
        if self.events.len() == before {
            None
        } else {
            Some(RepliesDelta::Removed(id.clone()))
        }
    }

    fn replace(&mut self, old_id: &EventId, new_event: &KernelEvent) -> Option<RepliesDelta> {
        if !self.accept(new_event) {
            return self.remove(old_id);
        }
        let pos = self.events.iter().position(|e| e.id == *old_id)?;
        self.events[pos] = new_event.clone();
        self.events.sort_by_key(|e| e.created_at);
        Some(RepliesDelta::Replaced {
            old_id: old_id.clone(),
            new_id: new_event.id.clone(),
        })
    }
}

/// Flat list of direct NIP-10 replies to a target event. Reactive deltas
/// fire as kind-1 events whose `refs.reply.id == spec.target` arrive.
pub struct RepliesView;

impl ViewModule for RepliesView {
    const NAMESPACE: &'static str = "nmp.nip01.replies";
    type Spec = RepliesSpec;
    type Payload = RepliesPayload;
    type Delta = RepliesDelta;
    type Key = EventId;
    type State = RepliesState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.target.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_SHORT_NOTE],
            tag_refs: vec![("e".into(), spec.target.clone())],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = RepliesState {
            target: spec.target.clone(),
            events: Vec::new(),
        };
        let payload = RepliesPayload {
            target_id: spec.target,
            replies: Vec::new(),
        };
        (state, payload)
    }

    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }

    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }

    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.replace(old, e)
    }

    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        RepliesPayload {
            target_id: state.target.clone(),
            replies: state.events.clone(),
        }
    }
}

// ─── ThreadView ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ThreadSpec {
    /// Hex event id of the thread root.
    pub root_event: EventId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ThreadNode {
    pub id: EventId,
    pub author: String,
    pub content: String,
    pub created_at: u64,
    pub parent_id: Option<EventId>,
    pub depth: u32,
    pub child_count: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ThreadPayload {
    pub root_event: EventId,
    pub nodes: Vec<ThreadNode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ThreadDelta {
    NodeAdded(EventId),
    NodeRemoved(EventId),
}

#[derive(Default)]
pub struct ThreadState {
    root: EventId,
    /// All known events keyed by id. We hold the full `KernelEvent` (cheap clone)
    /// so we can re-emit a flattened payload on demand.
    by_id: BTreeMap<EventId, KernelEvent>,
    /// Resolved parent for each known event id (`None` for the root itself).
    parent_of: BTreeMap<EventId, Option<EventId>>,
    /// Children index: parent_id → set of child event ids.
    children_of: BTreeMap<EventId, BTreeSet<EventId>>,
    /// Events that arrived before their parent (or before the root). Keyed by
    /// the missing parent id; stitched in when the parent later arrives.
    orphans: BTreeMap<EventId, BTreeSet<EventId>>,
}

impl ThreadState {
    fn link_into_tree(&mut self, event_id: &EventId, parent_id: Option<EventId>) {
        self.parent_of.insert(event_id.clone(), parent_id.clone());
        if let Some(p) = parent_id {
            self.children_of.entry(p).or_default().insert(event_id.clone());
        }
    }

    fn promote_orphans(&mut self, just_added: &EventId) {
        let waiting = self.orphans.remove(just_added);
        if let Some(waiting) = waiting {
            for child in waiting {
                self.link_into_tree(&child, Some(just_added.clone()));
                // Recursively promote anything waiting on `child`.
                let nested: Vec<EventId> = self
                    .orphans
                    .remove(&child)
                    .map(|s| s.into_iter().collect())
                    .unwrap_or_default();
                for grandchild in nested {
                    self.link_into_tree(&grandchild, Some(child.clone()));
                    self.promote_orphans(&grandchild);
                }
            }
        }
    }

    fn insert(&mut self, event: &KernelEvent) -> Option<ThreadDelta> {
        if event.kind != KIND_SHORT_NOTE {
            return None;
        }
        if self.by_id.contains_key(&event.id) {
            return None;
        }
        let record = try_from_kernel_event(event)?;

        // Determine the parent for tree-linking purposes.
        let parent_id: Option<EventId> = if event.id == self.root {
            None
        } else {
            record.refs.reply.map(|r| r.id)
        };

        // Reject events that are not in this thread:
        // - Not the root, AND not replying to anyone we know (yet), AND not
        //   replying to the root id directly: still might be ours later, but
        //   we only buffer if they reference the root somewhere in their refs.
        let in_thread = event.id == self.root
            || matches!(&parent_id, Some(p) if p == &self.root || self.by_id.contains_key(p));
        let could_join_later = parent_id.is_some() && !in_thread;

        if !in_thread && !could_join_later {
            return None;
        }

        self.by_id.insert(event.id.clone(), event.clone());

        if in_thread {
            self.link_into_tree(&event.id, parent_id);
            self.promote_orphans(&event.id);
            Some(ThreadDelta::NodeAdded(event.id.clone()))
        } else {
            // Buffer until parent arrives.
            let parent = parent_id.expect("could_join_later implies parent_id is Some");
            self.orphans.entry(parent).or_default().insert(event.id.clone());
            // Still added in `by_id` so a later parent insert can stitch it
            // in via promote_orphans without us needing the original event
            // again — but we emit no delta yet (no node is visible yet).
            None
        }
    }

    fn remove(&mut self, id: &EventId) -> Option<ThreadDelta> {
        let _event = self.by_id.remove(id)?;
        // Unlink from parent's children set.
        if let Some(Some(parent)) = self.parent_of.remove(id) {
            if let Some(siblings) = self.children_of.get_mut(&parent) {
                siblings.remove(id);
                if siblings.is_empty() {
                    self.children_of.remove(&parent);
                }
            }
        }
        // Any orphan waiting for this id no longer has a knowable parent —
        // drop them silently; they remain in `by_id` only if they were
        // already linked. Simpler: scan and drop.
        if let Some(children) = self.children_of.remove(id) {
            for c in children {
                self.by_id.remove(&c);
                self.parent_of.remove(&c);
            }
        }
        Some(ThreadDelta::NodeRemoved(id.clone()))
    }

    fn flatten(&self) -> Vec<ThreadNode> {
        // Root-first DFS. If the root hasn't arrived yet but some of its
        // children have, emit those children as a forest rooted at their
        // parents (still useful for partial UIs).
        let mut nodes = Vec::new();

        // Helper: depth-first walk from `id` at `depth`.
        fn walk(
            state: &ThreadState,
            id: &EventId,
            depth: u32,
            parent: Option<EventId>,
            out: &mut Vec<ThreadNode>,
        ) {
            let Some(event) = state.by_id.get(id) else { return };
            let child_count = state
                .children_of
                .get(id)
                .map(|s| s.len() as u32)
                .unwrap_or(0);
            out.push(ThreadNode {
                id: id.clone(),
                author: event.author.clone(),
                content: event.content.clone(),
                created_at: event.created_at,
                parent_id: parent,
                depth,
                child_count,
            });

            // Visit children sorted by created_at asc, then id for stability.
            if let Some(children) = state.children_of.get(id) {
                let mut sorted: Vec<&EventId> = children.iter().collect();
                sorted.sort_by_key(|cid| {
                    state
                        .by_id
                        .get(*cid)
                        .map(|e| (e.created_at, e.id.clone()))
                        .unwrap_or_default()
                });
                for c in sorted {
                    walk(state, c, depth + 1, Some(id.clone()), out);
                }
            }
        }

        if self.by_id.contains_key(&self.root) {
            walk(self, &self.root, 0, None, &mut nodes);
        } else {
            // Forest mode: emit subtrees we have (children of root we already
            // know about) so the UI doesn't have to special-case "root not
            // arrived yet". Each subtree is rooted at depth 0.
            let orphans_of_root: Vec<EventId> = self
                .parent_of
                .iter()
                .filter_map(|(id, p)| {
                    if p.as_deref() == Some(self.root.as_str()) {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for top in orphans_of_root {
                walk(self, &top, 0, None, &mut nodes);
            }
        }

        nodes
    }
}

/// Parent/child thread tree rooted at `spec.root_event`. Builds incrementally
/// as kind-1 events arrive; buffers out-of-order children until their parent
/// shows up (`orphans` table — applesauce's `parentReferences` pattern).
pub struct ThreadView;

impl ViewModule for ThreadView {
    const NAMESPACE: &'static str = "nmp.nip01.thread";
    type Spec = ThreadSpec;
    type Payload = ThreadPayload;
    type Delta = ThreadDelta;
    type Key = EventId;
    type State = ThreadState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.root_event.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_SHORT_NOTE],
            tag_refs: vec![("e".into(), spec.root_event.clone())],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = ThreadState {
            root: spec.root_event.clone(),
            ..ThreadState::default()
        };
        let payload = ThreadPayload {
            root_event: spec.root_event,
            nodes: Vec::new(),
        };
        (state, payload)
    }

    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }

    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }

    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        // Treat replace as remove + insert. Two deltas would be nicer but the
        // trait returns a single Option<Delta>; the kernel always re-emits a
        // fresh snapshot after a delta, so a single NodeAdded(new) suffices
        // for downstream correctness.
        let _ = s.remove(old);
        s.insert(e)
    }

    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        ThreadPayload {
            root_event: state.root.clone(),
            nodes: state.flatten(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ke(id: &str, author: &str, created_at: u64, tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
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
        let spec = RepliesSpec { target: "ROOT".into() };
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
        let spec = RepliesSpec { target: "ROOT".into() };
        let (mut state, _) = RepliesView::open(&ctx(), spec);
        let r_later = ke("LATER", "a", 20, vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]], "");
        let r_earlier = ke("EARLY", "a", 10, vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]], "");

        RepliesView::on_event_inserted(&ctx(), &mut state, &r_later);
        RepliesView::on_event_inserted(&ctx(), &mut state, &r_earlier);
        // Duplicate insert returns None.
        assert!(RepliesView::on_event_inserted(&ctx(), &mut state, &r_later).is_none());

        let snap = RepliesView::snapshot(&ctx(), &state);
        let ids: Vec<&str> = snap.replies.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["EARLY", "LATER"]);
    }

    #[test]
    fn replies_view_remove_clears_entry() {
        let spec = RepliesSpec { target: "ROOT".into() };
        let (mut state, _) = RepliesView::open(&ctx(), spec);
        let r = ke("R1", "a", 1, vec![vec!["e".into(), "ROOT".into(), "".into(), "reply".into()]], "");
        RepliesView::on_event_inserted(&ctx(), &mut state, &r);
        let delta = RepliesView::on_event_removed(&ctx(), &mut state, &"R1".to_string());
        assert!(matches!(delta, Some(RepliesDelta::Removed(_))));
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
        let spec = ThreadSpec { root_event: "R".into() };
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
        let spec = ThreadSpec { root_event: "R".into() };
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
        let spec = ThreadSpec { root_event: "RID".into() };
        let deps = ThreadView::dependencies(&spec);
        assert_eq!(deps.kinds, vec![KIND_SHORT_NOTE]);
        assert_eq!(deps.tag_refs, vec![("e".into(), "RID".into())]);
    }
}
