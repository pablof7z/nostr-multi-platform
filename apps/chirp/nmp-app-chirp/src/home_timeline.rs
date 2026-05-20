//! Chirp home timeline view module plus the temporary runtime adapter.
//!
//! `ChirpHomeTimelineView` is the app-owned `ViewModule` shape: it composes
//! the reusable NIP-10 modular timeline with the card metadata Chirp renders.
//! The `ChirpHomeTimelineRuntime` observer below is only the v1 bridge until
//! generated ViewBatch routing can open and drive app view modules directly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use nmp_core::KernelEventObserver;
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::{
    ModularTimelineDelta, ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState,
    Nip10ModularTimelineView,
};
use nmp_threading::ModulePolicy;
use serde::{Deserialize, Serialize};

use crate::payload::{ChirpEventCard, ChirpTimelineSnapshot};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChirpHomeTimelineSpec {
    pub viewer: Pubkey,
    #[serde(default)]
    pub kinds: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Pubkey>>,
    #[serde(default)]
    pub policy: ModulePolicy,
}

impl ChirpHomeTimelineSpec {
    pub fn for_viewer(viewer: Pubkey) -> Self {
        Self {
            viewer,
            kinds: Vec::new(),
            authors: None,
            policy: ModulePolicy::default(),
        }
    }

    fn modular_spec(&self) -> ModularTimelineSpec {
        ModularTimelineSpec {
            viewer: self.viewer.clone(),
            kinds: self.kinds.clone(),
            authors: self.authors.clone(),
            policy: self.policy.clone(),
        }
    }
}

impl From<ModularTimelineSpec> for ChirpHomeTimelineSpec {
    fn from(spec: ModularTimelineSpec) -> Self {
        Self {
            viewer: spec.viewer,
            kinds: spec.kinds,
            authors: spec.authors,
            policy: spec.policy,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ChirpHomeTimelineDelta {
    Timeline(ModularTimelineDelta),
    CardCached(EventId),
    CardRemoved(EventId),
}

pub struct ChirpHomeTimelineState {
    timeline: ModularTimelineState,
    cards: BTreeMap<EventId, ChirpEventCard>,
    accepted_kinds: BTreeSet<u32>,
    accepted_authors: Option<BTreeSet<Pubkey>>,
}

impl ChirpHomeTimelineState {
    fn admits(&self, event: &KernelEvent) -> bool {
        if !self.accepted_kinds.contains(&event.kind) {
            return false;
        }
        match &self.accepted_authors {
            Some(authors) => authors.contains(&event.author),
            None => true,
        }
    }
}

pub struct ChirpHomeTimelineView;

impl ViewModule for ChirpHomeTimelineView {
    const NAMESPACE: &'static str = "chirp.home_timeline";
    type Spec = ChirpHomeTimelineSpec;
    type Payload = ChirpTimelineSnapshot;
    type Delta = ChirpHomeTimelineDelta;
    type Key = String;
    type State = ChirpHomeTimelineState;

    fn key(spec: &Self::Spec) -> Self::Key {
        format!(
            "home:{}",
            <Nip10ModularTimelineView as ViewModule>::key(&spec.modular_spec())
        )
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        <Nip10ModularTimelineView as ViewModule>::dependencies(&spec.modular_spec())
    }

    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let modular_spec = spec.modular_spec();
        let accepted_kinds = modular_spec.effective_kinds().into_iter().collect();
        let accepted_authors = spec
            .authors
            .as_ref()
            .map(|authors| authors.iter().cloned().collect());
        let (timeline, payload) = <Nip10ModularTimelineView as ViewModule>::open(ctx, modular_spec);
        let state = ChirpHomeTimelineState {
            timeline,
            cards: BTreeMap::new(),
            accepted_kinds,
            accepted_authors,
        };
        (state, snapshot_from_parts(payload, &BTreeMap::new()))
    }

    fn on_event_inserted(
        ctx: &ViewContext,
        state: &mut Self::State,
        event: &KernelEvent,
    ) -> Option<Self::Delta> {
        let card_changed = if state.admits(event) {
            upsert_card(&mut state.cards, event)
        } else {
            false
        };
        let delta = <Nip10ModularTimelineView as ViewModule>::on_event_inserted(
            ctx,
            &mut state.timeline,
            event,
        );
        delta
            .map(ChirpHomeTimelineDelta::Timeline)
            .or_else(|| card_changed.then(|| ChirpHomeTimelineDelta::CardCached(event.id.clone())))
    }

    fn on_event_removed(
        ctx: &ViewContext,
        state: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        let card_removed = state.cards.remove(id).is_some();
        let delta = <Nip10ModularTimelineView as ViewModule>::on_event_removed(
            ctx,
            &mut state.timeline,
            id,
        );
        delta
            .map(ChirpHomeTimelineDelta::Timeline)
            .or_else(|| card_removed.then(|| ChirpHomeTimelineDelta::CardRemoved(id.clone())))
    }

    fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        state.cards.remove(old_id);
        let card_changed = if state.admits(new_event) {
            upsert_card(&mut state.cards, new_event)
        } else {
            false
        };
        let delta = <Nip10ModularTimelineView as ViewModule>::on_event_replaced(
            ctx,
            &mut state.timeline,
            old_id,
            new_event,
        );
        delta.map(ChirpHomeTimelineDelta::Timeline).or_else(|| {
            card_changed.then(|| ChirpHomeTimelineDelta::CardCached(new_event.id.clone()))
        })
    }

    fn on_projection_changed(
        ctx: &ViewContext,
        state: &mut Self::State,
        change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        <Nip10ModularTimelineView as ViewModule>::on_projection_changed(
            ctx,
            &mut state.timeline,
            change,
        )
        .map(ChirpHomeTimelineDelta::Timeline)
    }

    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        let payload = <Nip10ModularTimelineView as ViewModule>::snapshot(ctx, &state.timeline);
        snapshot_from_parts(payload, &state.cards)
    }
}

/// Compatibility owner for today's `KernelEventObserver` runtime.
///
/// Keep projection behavior out of this adapter. It exists because
/// `nmp-core` has the `ViewModule` trait contract, but not yet a generated
/// runtime that opens app modules and routes their payloads into ViewBatch.
pub struct ChirpHomeTimelineRuntime {
    inner: Mutex<ChirpHomeTimelineState>,
}

impl ChirpHomeTimelineRuntime {
    pub fn new(spec: impl Into<ChirpHomeTimelineSpec>) -> Self {
        let ctx = ViewContext::default();
        let (state, _payload) = <ChirpHomeTimelineView as ViewModule>::open(&ctx, spec.into());
        Self {
            inner: Mutex::new(state),
        }
    }

    pub fn snapshot(&self) -> ChirpTimelineSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return ChirpTimelineSnapshot::empty();
        };
        <ChirpHomeTimelineView as ViewModule>::snapshot(&ViewContext::default(), &inner)
    }
}

impl KernelEventObserver for ChirpHomeTimelineRuntime {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let _ = <ChirpHomeTimelineView as ViewModule>::on_event_inserted(
            &ViewContext::default(),
            &mut inner,
            event,
        );
    }
}

fn snapshot_from_parts(
    payload: ModularTimelinePayload,
    cards: &BTreeMap<EventId, ChirpEventCard>,
) -> ChirpTimelineSnapshot {
    ChirpTimelineSnapshot {
        blocks: payload.blocks,
        cards: cards.values().cloned().collect(),
    }
}

fn upsert_card(cards: &mut BTreeMap<EventId, ChirpEventCard>, event: &KernelEvent) -> bool {
    let card = ChirpEventCard::from(event);
    let changed = cards.get(&event.id) != Some(&card);
    cards.insert(event.id.clone(), card);
    changed
}

#[cfg(test)]
#[path = "home_timeline_tests.rs"]
mod home_timeline_tests;
