//! Dynamic observed-projection slot for active-account-derived feed sources.
//!
//! Some sources can be registered before a live acquisition shape exists. The
//! read model remains registered, but no observer interest is opened until the
//! source produces a concrete shape.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::ObservedProjectionCommandHandle;
use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_feed::TeardownAction;
use nmp_planner::InterestShape;

type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

#[derive(Clone)]
pub(super) struct DynamicObservedProjection {
    handle: ObservedProjectionCommandHandle,
    observer: Arc<dyn ObservedProjectionSink>,
    consumer_id: String,
    scope: u32,
    live_shape: LiveShape,
    replay_limit: usize,
    current: Arc<Mutex<Option<(InterestShape, ObservedProjectionId)>>>,
}

impl DynamicObservedProjection {
    pub(super) fn new(
        handle: ObservedProjectionCommandHandle,
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        live_shape: LiveShape,
        replay_limit: usize,
    ) -> Self {
        Self {
            handle,
            observer,
            consumer_id: consumer_id.into(),
            scope,
            live_shape,
            replay_limit,
            current: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn sync(&self) {
        let desired = (self.live_shape)();
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
            self.handle.close(id);
        }
        let Some(shape) = desired else {
            return;
        };
        let id = self.handle.open(ObservedProjection::from_shape(
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

    pub(super) fn current_id(&self) -> ObservedProjectionId {
        self.current
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|(_, id)| *id))
            .unwrap_or(ObservedProjectionId(0))
    }

    pub(super) fn teardown_action(&self) -> TeardownAction {
        let this = self.clone();
        Box::new(move || this.close_current())
    }

    fn close_current(&self) {
        let id = self
            .current
            .lock()
            .ok()
            .and_then(|mut current| current.take().map(|(_, id)| id));
        if let Some(id) = id {
            self.handle.close(id);
        }
    }
}
