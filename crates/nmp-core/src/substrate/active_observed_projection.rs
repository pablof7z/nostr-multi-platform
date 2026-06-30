//! Generic observed-projection reconciler.
//!
//! Reconciles a single registered observed projection against a live interest
//! shape on every [`sync`](ObservedProjectionReconciler::sync) call. When the
//! live shape changes the previous projection is closed and a new one is
//! opened; when the shape is `None` any current projection is closed and none
//! is opened. Successive `sync` calls with the same shape are idempotent.
//!
//! The reconciler is [`Clone`]-able via an inner `Arc`-wrapped state so all
//! clones share the same current-projection slot and only one open projection
//! exists at a time.
//!
//! D0-clean: this module carries no protocol nouns (no kind numbers, no NIP
//! names, no application-layer entity names).

use std::sync::{Arc, Mutex};

use super::{ObservedProjection, ObservedProjectionRegistrar};
use crate::{ObservedProjectionId, ObservedProjectionSink};
use nmp_planner::InterestShape;

type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

/// Reconciler that keeps exactly one observed projection open at a time,
/// driven by a caller-supplied live-shape closure.
///
/// # Lifecycle
///
/// - Call [`sync`](Self::sync) eagerly after construction and whenever the
///   live shape may have changed (e.g. on an identity-change callback or a
///   dependent-set change). The reconciler is idempotent: repeated `sync`
///   calls with the same shape are no-ops.
/// - Call [`close_current`](Self::close_current) to close the active
///   projection as part of a teardown sequence.
///
/// # Clone semantics
///
/// All clones share the same `Arc<Mutex<...>>` current-slot so `sync` or
/// `close_current` called on any clone affects the shared projection state.
/// This lets one clone live in an identity-change observer while another
/// lives in a follow-set `on_change` callback.
#[derive(Clone)]
pub struct ObservedProjectionReconciler {
    registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
    observer: Arc<dyn ObservedProjectionSink>,
    consumer_id: String,
    scope: u32,
    replay_limit: usize,
    live_shape: LiveShape,
    current: Arc<Mutex<Option<(InterestShape, ObservedProjectionId)>>>,
}

impl ObservedProjectionReconciler {
    pub fn new(
        registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        replay_limit: usize,
        live_shape: LiveShape,
    ) -> Self {
        Self {
            registrar,
            observer,
            consumer_id: consumer_id.into(),
            scope,
            replay_limit,
            live_shape,
            current: Arc::new(Mutex::new(None)),
        }
    }

    /// Reconcile the open observed projection against the live shape.
    ///
    /// No-op when the live shape matches the current slot. Otherwise closes
    /// the current projection (if any) and opens a new one for the new shape
    /// (if `Some`). The mutex is held for the duration to prevent concurrent
    /// `sync` calls from issuing duplicate opens.
    pub fn sync(&self) {
        // D15: host-supplied closure — wrap in catch_unwind so a panicking
        // live_shape cannot unwind the caller's dispatch loop.
        let desired =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.live_shape)()))
                .unwrap_or(None);
        let Ok(mut current) = self.current.lock() else {
            return;
        };
        if current
            .as_ref()
            .map(|(shape, _)| Some(shape) == desired.as_ref())
            .unwrap_or(desired.is_none())
        {
            return;
        }
        if let Some((_, id)) = current.take() {
            self.registrar.close_observed_projection(id);
        }
        let Some(shape) = desired else {
            return;
        };
        let id = self
            .registrar
            .open_observed_projection(ObservedProjection::from_shape(
                Arc::clone(&self.observer),
                self.consumer_id.clone(),
                self.scope,
                shape.clone(),
                self.replay_limit,
            ));
        if id.0 != 0 {
            *current = Some((shape, id));
        }
    }

    /// Return the id of the currently open projection, or
    /// `ObservedProjectionId(0)` when none is open.
    pub fn current_id(&self) -> ObservedProjectionId {
        self.current
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|(_, id)| *id))
            .unwrap_or(ObservedProjectionId(0))
    }

    /// Close the current projection and clear the slot.
    ///
    /// No-op when no projection is open. Suitable for use as a teardown
    /// action by wrapping in a closure: `Box::new(move || r.close_current())`.
    pub fn close_current(&self) {
        let id = self
            .current
            .lock()
            .ok()
            .and_then(|mut current| current.take().map(|(_, id)| id));
        if let Some(id) = id {
            self.registrar.close_observed_projection(id);
        }
    }
}
