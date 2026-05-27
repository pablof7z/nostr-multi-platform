//! `PublishStatusView` — the reactive projection of the publish engine.
//!
//! Per D5 the snapshot is bounded:
//! - `in_flight` carries every active publish (no upper bound — by definition
//!   bounded by what the app has dispatched).
//! - `recent_ok` and `recent_errors` are ring-buffer-bounded to keep payloads
//!   small and to honour the "snapshots bounded by what's open" rule.
//!
//! Per D8 the payload exposes `rev` for projection coalescing. The kernel
//! projection bridge owns the per-view emission budget.

use serde::{Deserialize, Serialize};

use super::action::{PublishHandle, RelayUrl};
use super::state::PerRelayState;
use super::traits::RelaySelectionReason;
use crate::substrate::{ProjectionChange, ViewContext, ViewDependencies};

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
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub per_relay: Vec<(RelayUrl, PerRelayState)>,
    /// Per-relay selection rationale, parallel to `per_relay` (same key set).
    /// Carried from the publish engine through the projection so kernel snapshots
    /// (`publish_outbox_items`) and apps can render "why was this relay targeted?"
    /// without re-running the outbox resolver. The `Vec<RelaySelectionReason>`
    /// shape captures the case where one canonical URL was selected for
    /// multiple reasons (e.g. a relay that is both the author's NIP-65 write
    /// relay AND a discovery indexer). Defaults to empty for
    /// backwards-compatible deserialization of older payloads.
    #[serde(default)]
    pub relay_reasons: Vec<(RelayUrl, Vec<RelaySelectionReason>)>,
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
    #[must_use]
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

    /// Replace the in-flight set wholesale when the engine refreshes status.
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

    /// Mark the snapshot as changed; the projection bridge enforces the D8
    /// `≤60 Hz per view` budget when it emits deltas.
    pub fn bump_rev(&mut self) {
        self.snapshot.rev = self.snapshot.rev.saturating_add(1);
    }
}

/// The reactive projection of the publish engine. Once an `impl ViewModule`,
/// now a plain type whose inherent methods are reached via static dispatch
/// (`PublishStatusView::open(...)`). No kernel-side `ViewRegistry` ever drove
/// the trait.
pub struct PublishStatusView;

impl PublishStatusView {
    pub const NAMESPACE: &'static str = "nmp.publish.status";

    #[must_use]
    pub fn key(_spec: &PublishStatusSpec) -> String {
        // Single global publish status view per app session.
        "nmp.publish.status:global".to_string()
    }

    #[must_use]
    pub fn dependencies(_spec: &PublishStatusSpec) -> ViewDependencies {
        // Publish status is driven by the engine via projection changes, not
        // by kernel-event subscription. The dependency surface is therefore
        // a single projection key.
        ViewDependencies {
            projection_keys: vec!["nmp.publish.status:global".to_string()],
            ..Default::default()
        }
    }

    #[must_use]
    pub fn open(
        _ctx: &ViewContext,
        spec: PublishStatusSpec,
    ) -> (PublishStatusState, PublishStatusSnapshot) {
        let state = PublishStatusState::new(&spec);
        let payload = state.snapshot.clone();
        (state, payload)
    }

    pub fn on_projection_changed(
        _ctx: &ViewContext,
        state: &mut PublishStatusState,
        change: &ProjectionChange,
    ) -> Option<PublishStatusSnapshot> {
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

    #[must_use]
    pub fn snapshot(_ctx: &ViewContext, state: &PublishStatusState) -> PublishStatusSnapshot {
        state.snapshot.clone()
    }
}
