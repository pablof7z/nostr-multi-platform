//! `RepostsView` — reposts (kinds 6 / 16) of a target, or by an author.
//!
//! `Spec` is a composite key with two variants: reposts *of* a target event,
//! or reposts *by* an author. Only kind:6 / kind:16 records surface here;
//! kind:7 reactions are filtered out (they belong to `ReactionSummaryView`).
//! A generic repost preserves its original `k` kind in the decoded record.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use serde::{Deserialize, Serialize};

use crate::decode::{ReactionTarget, ReactionRecord};
use crate::kinds::{KIND_GENERIC_REPOST, KIND_REPOST};

use super::accumulator::{ReactionAccumulator, ReactionViewDelta};

/// What to scope the reposts list to.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum RepostsSpec {
    /// Reposts of a specific target event/address.
    OfTarget(ReactionTarget),
    /// Reposts authored by a specific reposter pubkey.
    ByAuthor(String),
}

/// Always-renderable (D1): an empty `reposts` vec is a valid payload.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RepostsPayload {
    /// kind:6 / kind:16 records only, newest-first.
    pub reposts: Vec<ReactionRecord>,
}

/// Spec-scoped state. The shared accumulator keys on `event_id`; the scope
/// predicate rejects events that do not belong to this view (off-target or
/// off-author) so a misrouted repost can never surface (codex-finding-#2
/// analogue).
pub struct RepostsState {
    spec: RepostsSpec,
    inner: ReactionAccumulator,
}

impl RepostsState {
    fn event_in_scope(&self, event: &KernelEvent) -> bool {
        if event.kind != KIND_REPOST && event.kind != KIND_GENERIC_REPOST {
            return false;
        }
        let Some(record) = crate::decode::try_from_kernel_event(event) else {
            return false;
        };
        match &self.spec {
            RepostsSpec::OfTarget(t) => record.target == *t,
            RepostsSpec::ByAuthor(pk) => record.author == *pk,
        }
    }
}

pub struct RepostsView;
impl RepostsView {
    pub const NAMESPACE: &'static str = "nmp.reactions.reposts";

    pub fn key(spec: &RepostsSpec) -> RepostsSpec {
        spec.clone()
    }

    pub fn dependencies(spec: &RepostsSpec) -> ViewDependencies {
        let mut deps = ViewDependencies {
            kinds: vec![KIND_REPOST, KIND_GENERIC_REPOST],
            ..Default::default()
        };
        match spec {
            RepostsSpec::OfTarget(ReactionTarget::Event(id)) => {
                deps.tag_refs = vec![("e".to_string(), id.clone())];
            }
            RepostsSpec::OfTarget(ReactionTarget::Address(c)) => {
                deps.tag_refs = vec![(
                    "a".to_string(),
                    format!("{}:{}:{}", c.kind, c.pubkey, c.d_tag),
                )];
            }
            RepostsSpec::ByAuthor(pk) => {
                deps.authors = vec![pk.clone()];
            }
        }
        deps
    }

    pub fn open(_ctx: &ViewContext, spec: RepostsSpec) -> (RepostsState, RepostsPayload) {
        let state = RepostsState {
            spec,
            inner: ReactionAccumulator::default(),
        };
        (state, RepostsPayload::default())
    }

    pub fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut RepostsState,
        event: &KernelEvent,
    ) -> Option<ReactionViewDelta> {
        if !state.event_in_scope(event) {
            return None;
        }
        state.inner.insert(event)
    }

    pub fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut RepostsState,
        id: &EventId,
    ) -> Option<ReactionViewDelta> {
        state.inner.remove(id)
    }

    pub fn on_event_replaced(
        _ctx: &ViewContext,
        state: &mut RepostsState,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<ReactionViewDelta> {
        if !state.event_in_scope(new_event) {
            return state.inner.remove(old_id);
        }
        state.inner.replace(old_id, new_event)
    }

    pub fn snapshot(_ctx: &ViewContext, state: &RepostsState) -> RepostsPayload {
        // Every record in `inner` already passed the scope predicate (which
        // requires kind 6/16), so the snapshot is reposts-only by construction.
        RepostsPayload {
            reposts: state.inner.snapshot_records(),
        }
    }
}
