//! `ReactionSummaryView` — aggregate reactions for one target.
//!
//! The `Spec`/`Key` carries the [`ReactionTarget`] (a true composite
//! dependency key, mirroring nip23's `ArticleDetailView` coord key). State is
//! target-scoped: the shared [`ReactionAccumulator`] alone would admit a
//! misrouted reaction on a *different* target (the codex-finding-#2 analogue),
//! so we store the spec's target alongside it and reject any event whose
//! decoded target does not match — the view can never observe an off-target
//! reaction.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::ReactionTarget;

use super::accumulator::{ReactionAccumulator, ReactionViewDelta};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ReactionSummarySpec {
    /// The target whose reactions are aggregated.
    pub target: ReactionTarget,
}

/// D1 contract: the payload is always renderable — an empty summary (`total ==
/// 0`, no entries) is valid, never `Option::None`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ReactionSummaryPayload {
    /// `(content, count)` sorted by count desc then content asc. Only kind:7
    /// reaction content; reposts are surfaced by `RepostsView`.
    pub entries: Vec<(String, u64)>,
    /// Total distinct reactors after per-reactor newest-wins collapse.
    pub total: u64,
}

/// Target-scoped state. The shared accumulator keys on `event_id`; on its own
/// it would admit a reaction whose `e`/`a` tag points at a *different* target
/// and inflate this summary. We therefore reject any event whose decoded
/// target is not the spec's target.
pub struct SummaryState {
    target: ReactionTarget,
    inner: ReactionAccumulator,
}

impl SummaryState {
    fn event_matches_target(&self, event: &KernelEvent) -> bool {
        match crate::decode::try_from_kernel_event(event) {
            Some(record) => record.target == self.target,
            None => false,
        }
    }
}

/// Dependency tag-ref for a target: `("e", id)` for a concrete event, `("a",
/// "<kind>:<pubkey>:<dtag>")` for an addressable target. Mirrors nip23
/// `ArticleDetailView::dependencies`.
fn target_tag_ref(target: &ReactionTarget) -> (String, String) {
    match target {
        ReactionTarget::Event(id) => ("e".to_string(), id.clone()),
        ReactionTarget::Address(c) => (
            "a".to_string(),
            format!("{}:{}:{}", c.kind, c.pubkey, c.d_tag),
        ),
    }
}

pub struct ReactionSummaryView;
impl ViewModule for ReactionSummaryView {
    const NAMESPACE: &'static str = "nmp.reactions.summary";
    type Spec = ReactionSummarySpec;
    type Payload = ReactionSummaryPayload;
    type Delta = ReactionViewDelta;
    type Key = ReactionTarget;
    type State = SummaryState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.target.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![crate::kinds::KIND_REACTION],
            tag_refs: vec![target_tag_ref(&spec.target)],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = SummaryState {
            target: spec.target,
            inner: ReactionAccumulator::default(),
        };
        (state, ReactionSummaryPayload::default())
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !state.event_matches_target(event) {
            return None;
        }
        state.inner.insert(event)
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        state.inner.remove(id)
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        if !state.event_matches_target(new_event) {
            return state.inner.remove(old_id);
        }
        state.inner.replace(old_id, new_event)
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        let (entries, total) = state.inner.reaction_summary();
        ReactionSummaryPayload { entries, total }
    }
}
