//! `Nip22ModularTimelineView` — modular timeline over NIP-22 standalone
//! comments (kind:1111, non-NIP-29). Wraps the agnostic
//! [`nmp_threading::Grouper`] with a NIP-22 [`ParentResolver`] driven by
//! [`crate::try_from_kernel_event`].
//!
//! Symmetric with `nmp_nip01::Nip10ModularTimelineView`. Podcast UIs that
//! group episode-level comment threads instantiate this with
//! `tag_refs = [("E", episode_root_id)]` or `[("I", episode_url)]`.

use std::collections::BTreeSet;

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use nmp_threading::{
    GroupDelta, Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock,
};
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;
use crate::kinds::KIND_COMMENT;

pub type Pubkey = String;

/// `ParentResolver` over NIP-22 `CommentRecord`. Uses the existing decode
/// path so the (kind, h-tag) D4 discriminator is honored automatically —
/// NIP-29 kind:1111 events return `None` from `try_from_kernel_event` and
/// the grouper never sees them.
pub struct Nip22Resolver;

impl ParentResolver for Nip22Resolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        try_from_kernel_event(event).map(|r| r.parent)
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        try_from_kernel_event(event).map(|r| r.root)
    }

    fn parent_author(&self, _event: &KernelEvent) -> Option<String> {
        // NIP-22 doesn't pin the parent's author to a positional slot; the
        // wrapper leaves this `None` and lets UI clients resolve via the
        // parent event's own author field once the parent surfaces.
        None
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSpec {
    pub viewer: Pubkey,
    /// Defaults to `[1111]` when empty.
    #[serde(default)]
    pub kinds: Vec<u32>,
    /// Tag filter hints surfaced to the planner — e.g.
    /// `[("E", episode_root_id)]` to scope to an episode's comment tree, or
    /// `[("I", episode_url)]` for external-URI anchors.
    #[serde(default)]
    pub tag_refs: Vec<(String, String)>,
    #[serde(default)]
    pub policy: ModulePolicy,
}

impl ModularTimelineSpec {
    pub fn effective_kinds(&self) -> Vec<u32> {
        if self.kinds.is_empty() {
            vec![KIND_COMMENT]
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
    grouper: Grouper<Nip22Resolver>,
    accepted_kinds: BTreeSet<u32>,
}

impl ModularTimelineState {
    fn admits(&self, event: &KernelEvent) -> bool {
        if !self.accepted_kinds.contains(&event.kind) {
            return false;
        }
        // (kind, h-tag) D4 discriminator: h-tagged kind:1111 events
        // belong to nmp-nip29 (group comments). `try_from_kernel_event`
        // returns None for them; mirror `CommentsView::accept`'s guard
        // here so they never reach the grouper as headless events.
        try_from_kernel_event(event).is_some()
    }
}

pub struct Nip22ModularTimelineView;

impl ViewModule for Nip22ModularTimelineView {
    const NAMESPACE: &'static str = "nmp.nip22.modular_timeline";
    type Spec = ModularTimelineSpec;
    type Payload = ModularTimelinePayload;
    type Delta = ModularTimelineDelta;
    type Key = String;
    type State = ModularTimelineState;

    fn key(spec: &Self::Spec) -> Self::Key {
        let mut k = format!("{}|", spec.viewer);
        let mut kinds = spec.effective_kinds();
        kinds.sort_unstable();
        for kind in &kinds {
            k.push_str(&kind.to_string());
            k.push(',');
        }
        k.push('|');
        let mut tag_refs = spec.tag_refs.clone();
        tag_refs.sort();
        for (kk, vv) in tag_refs {
            k.push_str(&kk);
            k.push(':');
            k.push_str(&vv);
            k.push(',');
        }
        k
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: spec.effective_kinds(),
            tag_refs: spec.tag_refs.clone(),
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let accepted_kinds: BTreeSet<u32> = spec.effective_kinds().into_iter().collect();
        let state = ModularTimelineState {
            grouper: Grouper::new(Nip22Resolver, spec.policy.clone()),
            accepted_kinds,
        };
        (state, ModularTimelinePayload { blocks: Vec::new() })
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

    fn ke_comment(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1111,
            created_at: ts,
            tags,
            content: id.into(),
        }
    }

    fn top_level_uri_comment(id: &str, ts: u64, uri: &str) -> KernelEvent {
        ke_comment(
            id,
            ts,
            vec![
                vec!["I".into(), uri.into()],
                vec!["i".into(), uri.into()],
            ],
        )
    }

    fn nested_comment(id: &str, ts: u64, root_uri: &str, parent_comment_id: &str) -> KernelEvent {
        ke_comment(
            id,
            ts,
            vec![
                vec!["I".into(), root_uri.into()],
                vec!["e".into(), parent_comment_id.into()],
                vec!["k".into(), "1111".into()],
            ],
        )
    }

    #[test]
    fn defaults_to_kind_1111() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            tag_refs: vec![],
            policy: ModulePolicy::default(),
        };
        let deps = Nip22ModularTimelineView::dependencies(&spec);
        assert_eq!(deps.kinds, vec![1111]);
    }

    #[test]
    fn declares_tag_refs_for_planner_routing() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            tag_refs: vec![("E".into(), "EP".into()), ("I".into(), "https://x".into())],
            policy: ModulePolicy::default(),
        };
        let deps = Nip22ModularTimelineView::dependencies(&spec);
        assert_eq!(deps.tag_refs.len(), 2);
    }

    #[test]
    fn h_tagged_kind_1111_dropped_via_d4_discriminator() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            tag_refs: vec![],
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip22ModularTimelineView::open(&ctx(), spec);
        let h = ke_comment(
            "H",
            1,
            vec![
                vec!["I".into(), "uri".into()],
                vec!["h".into(), "group".into()],
            ],
        );
        // h-tagged kind:1111 belongs to nmp-nip29 (group comments). Mirror
        // CommentsView::accept's guard so they never reach the grouper.
        let delta = Nip22ModularTimelineView::on_event_inserted(&ctx(), &mut s, &h);
        assert!(delta.is_none());
        let snap = Nip22ModularTimelineView::snapshot(&ctx(), &s);
        assert!(snap.blocks.is_empty());
    }

    #[test]
    fn external_uri_top_level_yields_standalone() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            tag_refs: vec![("I".into(), "https://x".into())],
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip22ModularTimelineView::open(&ctx(), spec);
        let c = top_level_uri_comment("C1", 1, "https://x");
        let d = Nip22ModularTimelineView::on_event_inserted(&ctx(), &mut s, &c);
        assert!(matches!(d, Some(ModularTimelineDelta::BlockInserted(0))));
        let snap = Nip22ModularTimelineView::snapshot(&ctx(), &s);
        assert_eq!(snap.blocks.len(), 1);
        assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
    }

    #[test]
    fn nested_comment_extends_chain() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            tag_refs: vec![("I".into(), "https://x".into())],
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip22ModularTimelineView::open(&ctx(), spec);
        Nip22ModularTimelineView::on_event_inserted(
            &ctx(),
            &mut s,
            &top_level_uri_comment("C1", 1, "https://x"),
        );
        Nip22ModularTimelineView::on_event_inserted(
            &ctx(),
            &mut s,
            &nested_comment("C2", 2, "https://x", "C1"),
        );
        let snap = Nip22ModularTimelineView::snapshot(&ctx(), &s);
        assert_eq!(snap.blocks.len(), 1);
        match &snap.blocks[0] {
            TimelineBlock::Module { events, .. } => {
                assert_eq!(events, &vec!["C1".to_string(), "C2".to_string()]);
            }
            other => panic!("expected Module, got {other:?}"),
        }
    }
}
