//! `PublishStatusView` — the reactive projection of the publish engine.
//!
//! Per D5 the snapshot is bounded:
//! - `in_flight` carries every active publish (no upper bound — by definition
//!   bounded by what the app has dispatched).
//! - `recent_ok` and `recent_errors` are ring-buffer-bounded to keep payloads
//!   small and to honour the "snapshots bounded by what's open" rule.
//!
//! Per D8 each engine flush bumps `rev` exactly once even if many per-relay
//! acks landed in the same batch — the coalescer is the engine.

use serde::{Deserialize, Serialize};

use super::action::{PublishHandle, RelayUrl};
use super::state::PerRelayState;
use crate::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};

const DEFAULT_RECENT_OK_CAP: usize = 32;
const DEFAULT_RECENT_ERR_CAP: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishStatusSpec {
    /// Cap on `recent_ok` retained in the snapshot. 0 → use default.
    pub recent_ok_cap: usize,
    /// Cap on `recent_errors` retained in the snapshot. 0 → use default.
    pub recent_error_cap: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventPublishStatus {
    pub handle: PublishHandle,
    pub event_id: String,
    pub per_relay: Vec<(RelayUrl, PerRelayState)>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecentSuccess {
    pub handle: PublishHandle,
    pub event_id: String,
    pub accepted_by: Vec<RelayUrl>,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecentFailure {
    pub handle: PublishHandle,
    pub event_id: String,
    pub relay_url: RelayUrl,
    pub reason: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PublishStatusSnapshot {
    pub rev: u64,
    pub in_flight: Vec<EventPublishStatus>,
    pub recent_ok: Vec<RecentSuccess>,
    pub recent_errors: Vec<RecentFailure>,
}

/// View-module-side mutable state. The engine pushes via `apply_*` and the
/// snapshot is derived from it.
#[derive(Clone, Debug, Default)]
pub struct PublishStatusState {
    pub recent_ok_cap: usize,
    pub recent_err_cap: usize,
    pub snapshot: PublishStatusSnapshot,
}

impl PublishStatusState {
    pub fn new(spec: &PublishStatusSpec) -> Self {
        let recent_ok_cap = if spec.recent_ok_cap == 0 {
            DEFAULT_RECENT_OK_CAP
        } else {
            spec.recent_ok_cap
        };
        let recent_err_cap = if spec.recent_error_cap == 0 {
            DEFAULT_RECENT_ERR_CAP
        } else {
            spec.recent_error_cap
        };
        Self {
            recent_ok_cap,
            recent_err_cap,
            snapshot: PublishStatusSnapshot::default(),
        }
    }

    /// Replace the in-flight set wholesale; called once per coalesced batch.
    pub fn replace_in_flight(&mut self, rows: Vec<EventPublishStatus>) {
        self.snapshot.in_flight = rows;
    }

    pub fn push_success(&mut self, success: RecentSuccess) {
        self.snapshot.recent_ok.push(success);
        if self.snapshot.recent_ok.len() > self.recent_ok_cap {
            let overflow = self.snapshot.recent_ok.len() - self.recent_ok_cap;
            self.snapshot.recent_ok.drain(..overflow);
        }
    }

    pub fn push_failure(&mut self, failure: RecentFailure) {
        self.snapshot.recent_errors.push(failure);
        if self.snapshot.recent_errors.len() > self.recent_err_cap {
            let overflow = self.snapshot.recent_errors.len() - self.recent_err_cap;
            self.snapshot.recent_errors.drain(..overflow);
        }
    }

    /// One bump per coalesced flush — the engine calls exactly once per batch
    /// per D8 (`≤60 Hz per view`).
    pub fn bump_rev(&mut self) {
        self.snapshot.rev = self.snapshot.rev.saturating_add(1);
    }
}

pub struct PublishStatusView;

impl ViewModule for PublishStatusView {
    const NAMESPACE: &'static str = "nmp.publish.status";

    type Spec = PublishStatusSpec;
    type Payload = PublishStatusSnapshot;
    type Delta = PublishStatusSnapshot;
    type Key = String;
    type State = PublishStatusState;

    fn key(_spec: &Self::Spec) -> Self::Key {
        // Single global publish status view per app session.
        "nmp.publish.status:global".to_string()
    }

    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        // Publish status is driven by the engine via projection changes, not
        // by kernel-event subscription. The dependency surface is therefore
        // a single projection key.
        ViewDependencies {
            kinds: Vec::new(),
            authors: Vec::new(),
            ids: Vec::new(),
            tag_refs: Vec::new(),
            projection_keys: vec!["nmp.publish.status:global".to_string()],
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = PublishStatusState::new(&spec);
        let payload = state.snapshot.clone();
        (state, payload)
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _id: &EventId,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _old_id: &EventId,
        _new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        if change.namespace != Self::NAMESPACE {
            return None;
        }
        if let Ok(snapshot) =
            serde_json::from_value::<PublishStatusSnapshot>(change.payload.clone())
        {
            state.snapshot = snapshot.clone();
            Some(snapshot)
        } else {
            None
        }
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        state.snapshot.clone()
    }
}
