//! `Nip10ModularTimelineView` — Chirp-style modular timeline over NIP-10
//! kind:1 events. Wraps the agnostic [`nmp_threading::Grouper`] with a
//! NIP-10 [`ParentResolver`] driven by [`nmp_core::tags::parse_nip10`].
//!
//! Replaces the doctrine-violating `MetaTimelineViewModule` mention in
//! `nmp-core/src/planner/interest.rs` — that work now lives here in a
//! sibling protocol crate per ADR-0009.

use std::collections::BTreeSet;

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use nmp_core::tags::parse_nip10;
use nmp_threading::{
    GroupDelta, Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock,
};
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;

/// Pubkey alias mirroring the planner.
pub type Pubkey = String;

/// `ParentResolver` over NIP-10 markers. The grouper never sees kind:1
/// directly — it only sees this resolver's `ThreadPointer` answers.
pub struct Nip10Resolver;

impl ParentResolver for Nip10Resolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        let refs = parse_nip10(&event.tags);
        refs.reply.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        let refs = parse_nip10(&event.tags);
        refs.root.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn parent_author(&self, event: &KernelEvent) -> Option<String> {
        let refs = parse_nip10(&event.tags);
        // Best-effort: NIP-10 says the participants' p-tags accompany the
        // reply, but there's no positional guarantee that the first p-tag
        // is the parent's author. Return the first p-tag — callers treat
        // this as a hint, not authoritative.
        refs.mentioned_pubkeys.into_iter().next()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSpec {
    /// Viewer pubkey — included for future mute / personalization keys.
    pub viewer: Pubkey,
    /// Event kinds to admit. Defaults to `[1]` when empty.
    #[serde(default)]
    pub kinds: Vec<u32>,
    /// Author filter. `None` accepts any author the planner surfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Pubkey>>,
    /// Grouping policy.
    #[serde(default)]
    pub policy: ModulePolicy,
}

impl ModularTimelineSpec {
    pub fn effective_kinds(&self) -> Vec<u32> {
        if self.kinds.is_empty() {
            vec![KIND_SHORT_NOTE]
        } else {
            self.kinds.clone()
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelinePayload {
    pub blocks: Vec<TimelineBlock>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ModularTimelineDelta {
    BlockInserted(usize),
    BlockReplaced(usize),
    BlockRemoved(usize),
}

impl From<GroupDelta> for ModularTimelineDelta {
    fn from(d: GroupDelta) -> Self {
        match d {
            GroupDelta::BlockInserted(i) => ModularTimelineDelta::BlockInserted(i),
            GroupDelta::BlockReplaced(i) => ModularTimelineDelta::BlockReplaced(i),
            GroupDelta::BlockRemoved(i) => ModularTimelineDelta::BlockRemoved(i),
        }
    }
}

pub struct ModularTimelineState {
    grouper: Grouper<Nip10Resolver>,
    accepted_kinds: BTreeSet<u32>,
    accepted_authors: Option<BTreeSet<Pubkey>>,
}

impl ModularTimelineState {
    fn admits(&self, event: &KernelEvent) -> bool {
        if !self.accepted_kinds.contains(&event.kind) {
            return false;
        }
        if let Some(auth) = &self.accepted_authors {
            if !auth.contains(&event.author) {
                return false;
            }
        }
        true
    }
}

/// `ViewModule` impl, exported as a public type for per-app composition.
pub struct Nip10ModularTimelineView;

impl ViewModule for Nip10ModularTimelineView {
    const NAMESPACE: &'static str = "nmp.nip01.modular_timeline";
    type Spec = ModularTimelineSpec;
    type Payload = ModularTimelinePayload;
    type Delta = ModularTimelineDelta;
    type Key = String;
    type State = ModularTimelineState;

    fn key(spec: &Self::Spec) -> Self::Key {
        // One open view per (viewer, author-set, kinds) tuple.
        let mut k = format!("{}|", spec.viewer);
        let mut kinds = spec.effective_kinds();
        kinds.sort_unstable();
        for kind in &kinds {
            k.push_str(&kind.to_string());
            k.push(',');
        }
        k.push('|');
        if let Some(authors) = &spec.authors {
            let mut sorted = authors.clone();
            sorted.sort();
            for a in sorted {
                k.push_str(&a);
                k.push(',');
            }
        }
        k
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: spec.effective_kinds(),
            authors: spec.authors.clone().unwrap_or_default(),
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let accepted_kinds: BTreeSet<u32> = spec.effective_kinds().into_iter().collect();
        let accepted_authors = spec
            .authors
            .as_ref()
            .map(|v| v.iter().cloned().collect::<BTreeSet<_>>());
        let state = ModularTimelineState {
            grouper: Grouper::new(Nip10Resolver, spec.policy.clone()),
            accepted_kinds,
            accepted_authors,
        };
        let payload = ModularTimelinePayload { blocks: Vec::new() };
        (state, payload)
    }

    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !s.admits(e) {
            return None;
        }
        s.grouper.on_insert(e).map(Into::into)
    }

    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.grouper.on_remove(id).map(Into::into)
    }

    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !s.admits(e) {
            return s.grouper.on_remove(old).map(Into::into);
        }
        s.grouper.on_replace(old, e).map(Into::into)
    }

    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        // Mute / hide projections land in a follow-up. The wrapper filters
        // at `admits()` once a projection key is wired into Spec.
        None
    }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        ModularTimelinePayload {
            blocks: state.grouper.blocks().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
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
        let (_state, payload) = Nip10ModularTimelineView::open(&ctx(), spec);
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
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let root = note("R", 1, vec![]);
        let reply = marked("C", 2, "R", "R");
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &root);
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &reply);
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
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let mut e = note("Z", 1, vec![]);
        e.kind = 30023;
        assert!(Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &e).is_none());
        assert!(Nip10ModularTimelineView::snapshot(&ctx(), &s).blocks.is_empty());
    }

    #[test]
    fn author_filter_excludes_others() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: Some(vec!["alice".into()]),
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let mut a = note("A", 1, vec![]);
        a.author = "alice".into();
        let mut b = note("B", 2, vec![]);
        b.author = "bob".into();
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &a);
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &b);
        let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);
        assert_eq!(snap.blocks.len(), 1);
    }
}
