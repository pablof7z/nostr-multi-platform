//! Dynamic observed-projection slot for active-account-derived feed sources.
//!
//! Some sources can be registered before a live acquisition shape exists. The
//! read model remains registered, but no observer interest is opened until the
//! source produces a concrete shape.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::ObservedProjection;
use nmp_core::substrate::ObservedProjectionCommandHandle;
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_feed::TeardownAction;
use nmp_planner::InterestShape;

type LiveShapes = Arc<dyn Fn() -> Vec<InterestShape> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct DynamicObservedProjectionSet {
    handle: ObservedProjectionCommandHandle,
    observer: Arc<dyn ObservedProjectionSink>,
    consumer_id: String,
    scope: u32,
    live_shapes: LiveShapes,
    replay_limit: usize,
    current: Arc<Mutex<Vec<(InterestShape, ObservedProjectionId)>>>,
}

impl DynamicObservedProjectionSet {
    pub(crate) fn new(
        handle: ObservedProjectionCommandHandle,
        observer: Arc<dyn ObservedProjectionSink>,
        consumer_id: impl Into<String>,
        scope: u32,
        live_shapes: LiveShapes,
        replay_limit: usize,
    ) -> Self {
        Self {
            handle,
            observer,
            consumer_id: consumer_id.into(),
            scope,
            live_shapes,
            replay_limit,
            current: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn sync(&self) {
        let desired = dedupe_shapes((self.live_shapes)());
        let Ok(mut current) = self.current.lock() else {
            return;
        };
        if same_shape_set(&current, &desired) {
            return;
        }
        for (_, id) in current.drain(..) {
            self.handle.close(id);
        }
        for shape in desired {
            let id = self.handle.open(ObservedProjection::from_shape(
                Arc::clone(&self.observer),
                self.consumer_id.clone(),
                self.scope,
                shape.clone(),
                self.replay_limit,
            ));
            if id.0 != 0 {
                current.push((shape, id));
            }
        }
    }

    pub(crate) fn teardown_action(&self) -> TeardownAction {
        let this = self.clone();
        Box::new(move || this.close_current())
    }

    fn close_current(&self) {
        let ids = self
            .current
            .lock()
            .map(|mut current| current.drain(..).map(|(_, id)| id).collect::<Vec<_>>())
            .unwrap_or_default();
        for id in ids {
            self.handle.close(id);
        }
    }
}

fn dedupe_shapes(shapes: Vec<InterestShape>) -> Vec<InterestShape> {
    let mut out = Vec::new();
    for shape in shapes {
        if !out.contains(&shape) {
            out.push(shape);
        }
    }
    out
}

fn same_shape_set(
    current: &[(InterestShape, ObservedProjectionId)],
    desired: &[InterestShape],
) -> bool {
    current.len() == desired.len()
        && current
            .iter()
            .map(|(shape, _)| shape)
            .zip(desired)
            .all(|(left, right)| left == right)
}
