//! Kernel-event ingest drives embed resolution.
//!
//! The view shape is callback-driven and singleton. Event ingest updates the
//! `resolved` payload of *already-claimed* targets; removal clears stale
//! resolutions for both event-id and coordinate-addressed (`naddr`) embeds.

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use serde::{Deserialize, Serialize};

use super::state::EmbedClaimState;
use super::target::{EmbedTarget, ResolvedEvent};
use super::EmbedClaimRegistry;

/// Open-spec for the registry view — apps don't actually open one
/// `EmbedClaimRegistry` per "view"; the spec is unit-shaped. The registry is
/// conceptually app-singleton, with `claim`/`release` driving its state.
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
        entry.resolved.clone_from(&resolved);
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
        for (target, entry) in &state.entries {
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

impl EmbedClaimRegistry {
    /// View key — the registry is an app-singleton, so the key is unit.
    pub fn key(_spec: &EmbedClaimSpec) {}

    /// Event dependencies for this view. Unit-shaped spec → none declared.
    #[must_use] 
    pub fn dependencies(_spec: &EmbedClaimSpec) -> ViewDependencies {
        // The dependency contract is spec-driven and static; the spec here is
        // unit-shaped, so there is nothing to declare. Kernel-side
        // claim-driven subscription wiring is a Phase-2 seam (see the
        // crate-level docs on `EmbedClaimRegistry`); this module delivers
        // in-memory claim dedupe only, not upstream fetch.
        ViewDependencies::default()
    }

    /// Open a fresh registry view, returning its empty state + snapshot.
    #[must_use] 
    pub fn open(
        _ctx: &ViewContext,
        _spec: EmbedClaimSpec,
    ) -> (EmbedClaimState, EmbedRegistrySnapshot) {
        let state = EmbedClaimRegistry::state();
        let payload = Self::snapshot_state(&state);
        (state, payload)
    }

    /// Ingest an inserted kernel event — resolves any claimed target it
    /// matches (direct event-id or coordinate-addressed).
    #[must_use]
    pub fn on_event_inserted(
        _ctx: &ViewContext,
        state: &mut EmbedClaimState,
        event: &KernelEvent,
    ) -> Option<EmbedClaimDelta> {
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

    /// Ingest a removed kernel event — clears stale resolutions for any
    /// claimed target that resolved to `id`.
    #[must_use]
    pub fn on_event_removed(
        _ctx: &ViewContext,
        state: &mut EmbedClaimState,
        id: &EventId,
    ) -> Option<EmbedClaimDelta> {
        Self::clear_resolutions_for(state, id)
    }

    /// Ingest a replaced kernel event — remove + insert.
    #[must_use]
    pub fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut EmbedClaimState,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<EmbedClaimDelta> {
        let _ = Self::on_event_removed(ctx, state, old_id);
        Self::on_event_inserted(ctx, state, new_event)
    }

    /// Outward-facing snapshot of the registry state.
    #[must_use] 
    pub fn snapshot(_ctx: &ViewContext, state: &EmbedClaimState) -> EmbedRegistrySnapshot {
        Self::snapshot_state(state)
    }
}
