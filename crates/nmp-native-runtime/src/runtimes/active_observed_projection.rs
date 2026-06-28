//! Active-account observed-projection reconciler.
//!
//! Some protocol read models are keyed by the active account's own events
//! (kind:3, kind:10000, kind:10003, kind:10007). They must not open a broad
//! kind-only observer before sign-in and then filter by author later: that
//! replays cached events while the active pubkey is still unknown, and it is
//! the same over-broad observer shape the filterless observer removal is meant
//! to eliminate. This helper opens the observed projection only when the active
//! pubkey is known, using a concrete `authors=[active]` shape.

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_planner::InterestShape;

type ShapeForPubkey = Arc<dyn Fn(&str) -> InterestShape + Send + Sync>;

pub(crate) struct ActiveObservedProjection {
    active_pubkey: ActiveAccountSlot,
    registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
    observer: Arc<dyn ObservedProjectionSink>,
    consumer_id: String,
    scope: u32,
    replay_limit: usize,
    shape_for_pubkey: ShapeForPubkey,
    current: Mutex<Option<(String, ObservedProjectionId)>>,
}

impl ActiveObservedProjection {
    pub(crate) fn new(
        active_pubkey: ActiveAccountSlot,
        registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        replay_limit: usize,
        shape_for_pubkey: ShapeForPubkey,
    ) -> Self {
        Self {
            active_pubkey,
            registrar,
            observer,
            consumer_id: consumer_id.into(),
            scope,
            replay_limit,
            shape_for_pubkey,
            current: Mutex::new(None),
        }
    }

    pub(crate) fn sync(&self) {
        let active = read_active(&self.active_pubkey);
        let previous = {
            let Ok(mut current) = self.current.lock() else {
                return;
            };
            if current
                .as_ref()
                .map(|(pubkey, _)| Some(pubkey.as_str()) == active.as_deref())
                .unwrap_or(active.is_none())
            {
                return;
            }
            current.take()
        };

        if let Some((_, id)) = previous {
            self.registrar.close_observed_projection(id);
        }

        let Some(pubkey) = active else {
            return;
        };
        let shape = (self.shape_for_pubkey)(&pubkey);
        let id = self
            .registrar
            .open_observed_projection(ObservedProjection::from_shape(
                Arc::clone(&self.observer),
                self.consumer_id.clone(),
                self.scope,
                shape,
                self.replay_limit,
            ));
        if id.0 == 0 {
            return;
        }

        if let Ok(mut current) = self.current.lock() {
            *current = Some((pubkey, id));
        }
    }

    pub(crate) fn current_id(&self) -> ObservedProjectionId {
        self.current
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|(_, id)| *id))
            .unwrap_or(ObservedProjectionId(0))
    }
}

fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}
