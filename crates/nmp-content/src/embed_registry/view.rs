//! `ViewModule` adapter — kernel-event ingest drives embed resolution.
//!
//! The trait shape is callback-driven and singleton (`Key = ()`). Event
//! ingest updates the `resolved` payload of *already-claimed* targets;
//! removal clears stale resolutions for both event-id and
//! coordinate-addressed (`naddr`) embeds.

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use super::state::EmbedClaimState;
use super::target::{EmbedTarget, ResolvedEvent};
use super::EmbedClaimRegistry;

/// `ViewModule::Spec` — apps don't actually open one `EmbedClaimRegistry`
/// per "view"; the spec is unit-shaped. The registry is conceptually
/// app-singleton, with `claim`/`release` driving its state. We satisfy the
/// `ViewModule` trait for ADR-0009 conformance.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct EmbedClaimSpec;

/// Outward-facing snapshot of the registry — claimed targets + their
/// resolved payloads (when present).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EmbedRegistrySnapshot {
    /// Each entry: (target, refcount, optional resolved event).
    pub entries: Vec<(EmbedTarget, usize, Option<ResolvedEvent>)>,
}

/// Delta emitted on registry change.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EmbedClaimDelta {
    /// A target's refcount or resolution changed.
    Updated {
        /// The affected target.
        target: EmbedTarget,
        /// Current refcount (0 = released).
        refcount: usize,
        /// Resolved event, if any.
        resolved: Option<ResolvedEvent>,
    },
}

impl EmbedClaimRegistry {
    pub(super) fn snapshot_state(state: &EmbedClaimState) -> EmbedRegistrySnapshot {
        let entries = state
            .entries
            .iter()
            .map(|(t, e)| (t.clone(), e.refcount(), e.resolved.clone()))
            .collect();
        EmbedRegistrySnapshot { entries }
    }

    fn update_resolution(
        state: &mut EmbedClaimState,
        target: &EmbedTarget,
        resolved: Option<ResolvedEvent>,
    ) -> Option<EmbedClaimDelta> {
        let entry = state.entries.get_mut(target)?;
        let same = entry.resolved == resolved;
        entry.resolved = resolved.clone();
        if same {
            None
        } else {
            Some(EmbedClaimDelta::Updated {
                target: target.clone(),
                refcount: entry.refcount(),
                resolved,
            })
        }
    }

    /// Clear the resolution of every claimed target currently resolved to
    /// `id` — both the direct `Event(id)` target and any coordinate
    /// (`Address`) target whose `resolved.id` matches. Returns a single
    /// delta for the first cleared target (the kernel re-delivers the
    /// replacement via a subsequent insert, which emits its own delta).
    fn clear_resolutions_for(
        state: &mut EmbedClaimState,
        id: &EventId,
    ) -> Option<EmbedClaimDelta> {
        let mut affected: Vec<EmbedTarget> = Vec::new();
        if state.entries.contains_key(&EmbedTarget::Event(id.clone())) {
            affected.push(EmbedTarget::Event(id.clone()));
        }
        for (target, entry) in state.entries.iter() {
            if matches!(target, EmbedTarget::Address { .. })
                && entry.resolved.as_ref().is_some_and(|r| &r.id == id)
            {
                affected.push(target.clone());
            }
        }
        let mut delta = None;
        for target in affected {
            let d = Self::update_resolution(state, &target, None);
            delta = delta.or(d);
        }
        delta
    }
}

impl ViewModule for EmbedClaimRegistry {
    const NAMESPACE: &'static str = "nmp.content.embed_registry";

    type Spec = EmbedClaimSpec;
    type Payload = EmbedRegistrySnapshot;
    type Delta = EmbedClaimDelta;
    type Key = (); // singleton
    type State = EmbedClaimState;

    fn key(_spec: &Self::Spec) -> Self::Key {}

    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        // The `ViewModule` dependency contract is spec-driven and static;
        // the spec here is unit-shaped, so there is nothing to declare.
        // Kernel-side claim-driven subscription wiring is a Phase-2 seam
        // (see the crate-level docs on `EmbedClaimRegistry`); this module
        // delivers in-memory claim dedupe only, not upstream fetch.
        ViewDependencies::default()
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = EmbedClaimRegistry::state();
        let payload = Self::snapshot_state(&state);
        (state, payload)
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        // If this event is currently claimed, update its resolution.
        let target = EmbedTarget::Event(event.id.clone());
        if state.entries.contains_key(&target) {
            return Self::update_resolution(state, &target, Some(event.into()));
        }
        // Address-coordinated: look for an Address target matching
        // (kind, author, d-tag).
        let d_tag = event
            .tags
            .iter()
            .find(|t| t.len() >= 2 && t[0] == "d")
            .map(|t| t[1].clone());
        if let Some(identifier) = d_tag {
            let target = EmbedTarget::Address {
                kind: event.kind,
                pubkey: event.author.clone(),
                identifier,
            };
            if state.entries.contains_key(&target) {
                return Self::update_resolution(state, &target, Some(event.into()));
            }
        }
        None
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        Self::clear_resolutions_for(state, id)
    }

    fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        let _ = Self::on_event_removed(ctx, state, old_id);
        Self::on_event_inserted(ctx, state, new_event)
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        Self::snapshot_state(state)
    }
}
