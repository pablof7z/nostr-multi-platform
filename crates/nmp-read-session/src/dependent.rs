//! Engine-owned dependent observed-demand reconciliation.

use std::sync::{Arc, Mutex, Weak};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};

use crate::host::{
    ReadDependentDemand, ReadDependentDemandProvider, ReadHost, ReadInterestController,
};

pub(crate) fn prepare_dependent_demand_observer(
    host: &dyn ReadHost,
    key: &str,
    observer: Arc<dyn ObservedProjectionSink>,
    providers: Vec<ReadDependentDemandProvider>,
) -> (
    Arc<dyn ObservedProjectionSink>,
    Vec<Arc<DependentDemandReconciler>>,
) {
    if providers.is_empty() {
        return (observer, Vec::new());
    }
    let Some(controller) = host.read_interest_controller() else {
        return (observer, Vec::new());
    };

    let wrapper = Arc::new(DependentDemandObserver::new(observer));
    let weak_wrapper = Arc::downgrade(&wrapper);
    let reconcilers = providers
        .into_iter()
        .enumerate()
        .map(|(idx, provider)| {
            Arc::new(DependentDemandReconciler::new(
                provider,
                controller.clone(),
                weak_wrapper.clone(),
                format!("{key}.dependent.{idx}"),
            ))
        })
        .collect::<Vec<_>>();
    wrapper.set_reconcilers(reconcilers.clone());
    (wrapper, reconcilers)
}

pub(crate) fn close_dependent_reconcilers(reconcilers: &[Arc<DependentDemandReconciler>]) {
    for reconciler in reconcilers {
        reconciler.close_current();
    }
}

pub(crate) struct DependentDemandReconciler {
    provider: ReadDependentDemandProvider,
    controller: ReadInterestController,
    observer: Weak<DependentDemandObserver>,
    consumer_id: String,
    current: Mutex<Option<(ReadDependentDemand, ObservedProjectionId)>>,
    sync_state: Mutex<DependentDemandSyncState>,
}

struct DependentDemandObserver {
    inner: Arc<dyn ObservedProjectionSink>,
    reconcilers: Mutex<Vec<Arc<DependentDemandReconciler>>>,
}

#[derive(Default)]
struct DependentDemandSyncState {
    active: bool,
    dirty: bool,
}

impl DependentDemandObserver {
    fn new(inner: Arc<dyn ObservedProjectionSink>) -> Self {
        Self {
            inner,
            reconcilers: Mutex::new(Vec::new()),
        }
    }

    fn set_reconcilers(&self, reconcilers: Vec<Arc<DependentDemandReconciler>>) {
        if let Ok(mut slot) = self.reconcilers.lock() {
            *slot = reconcilers;
        }
    }

    fn sync_dependent_demands(&self) {
        let reconcilers = self
            .reconcilers
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default();
        for reconciler in reconcilers {
            reconciler.sync();
        }
    }
}

impl ObservedProjectionSink for DependentDemandObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.inner.on_kernel_event(event);
        self.sync_dependent_demands();
    }
}

impl DependentDemandReconciler {
    fn new(
        provider: ReadDependentDemandProvider,
        controller: ReadInterestController,
        observer: Weak<DependentDemandObserver>,
        consumer_id: String,
    ) -> Self {
        Self {
            provider,
            controller,
            observer,
            consumer_id,
            current: Mutex::new(None),
            sync_state: Mutex::new(DependentDemandSyncState::default()),
        }
    }

    fn sync(&self) {
        if !self.begin_sync() {
            return;
        }
        loop {
            self.sync_once();
            if !self.finish_or_continue_sync() {
                break;
            }
        }
    }

    fn begin_sync(&self) -> bool {
        let Ok(mut state) = self.sync_state.lock() else {
            return false;
        };
        if state.active {
            state.dirty = true;
            return false;
        }
        state.active = true;
        state.dirty = false;
        true
    }

    fn finish_or_continue_sync(&self) -> bool {
        let Ok(mut state) = self.sync_state.lock() else {
            return false;
        };
        if state.dirty {
            state.dirty = false;
            true
        } else {
            state.active = false;
            false
        }
    }

    fn sync_once(&self) {
        let desired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.provider)()))
            .unwrap_or(None);
        let old_id = {
            let Ok(mut current) = self.current.lock() else {
                return;
            };
            if current
                .as_ref()
                .map(|(demand, _)| Some(demand) == desired.as_ref())
                .unwrap_or(desired.is_none())
            {
                return;
            }
            current.take().map(|(_, id)| id)
        };

        if let Some(id) = old_id {
            self.controller.close(id);
        }
        let Some(demand) = desired else {
            return;
        };
        let Some(observer) = self.observer.upgrade() else {
            return;
        };
        let id = self.controller.open(ObservedProjection::from_shape(
            observer,
            self.consumer_id.clone(),
            demand.scope,
            demand.shape.clone(),
            demand.replay_limit,
        ));
        if id.0 != 0 {
            if let Ok(mut current) = self.current.lock() {
                *current = Some((demand, id));
            }
        }
    }

    pub(crate) fn close_current(&self) {
        let id = self
            .current
            .lock()
            .ok()
            .and_then(|mut current| current.take().map(|(_, id)| id));
        if let Some(id) = id {
            self.controller.close(id);
        }
    }
}
