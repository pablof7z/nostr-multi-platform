use super::*;
use crate::substrate::KernelEvent;
use std::cell::RefCell;
use std::sync::Arc;

/// Fake registrar that records which calls were made and in what order.
struct FakeRegistrar {
    calls: RefCell<Vec<&'static str>>,
    next_id: RefCell<u64>,
}

impl FakeRegistrar {
    fn new() -> Self {
        FakeRegistrar {
            calls: RefCell::new(Vec::new()),
            next_id: RefCell::new(1),
        }
    }

    fn new_with_zero_id() -> Self {
        FakeRegistrar {
            calls: RefCell::new(Vec::new()),
            next_id: RefCell::new(0),
        }
    }

    fn recorded(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl EventObserverRegistrar for FakeRegistrar {
    fn register_event_observer(
        &self,
        _observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        let id = *self.next_id.borrow();
        *self.next_id.borrow_mut() = id + 1;
        self.calls.borrow_mut().push("register_event_observer");
        KernelEventObserverId(id)
    }

    fn unregister_event_observer(&self, _id: KernelEventObserverId) {}

    fn swap_singleton_event_observer(
        &self,
        _new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        None
    }
}

impl SnapshotProjectionRegistrar for FakeRegistrar {
    fn register_typed_snapshot_projection<K, F>(&self, _key: K, _f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
        self.calls.borrow_mut().push("register_typed_snapshot_projection");
    }

    fn register_snapshot_tick_observer<F>(&self, _f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
    }

    fn declare_incremental_apply(
        &self,
    ) -> Result<(), super::projection::IncrementalApplyError> {
        Ok(())
    }

    fn incremental_apply_handle(
        &self,
    ) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    fn frame_identity_handles(
        &self,
    ) -> (
        std::sync::Arc<std::sync::atomic::AtomicU64>,
        std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        (
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        )
    }

    fn remove_snapshot_projection(&self, _key: &str) {}

    fn declare_consumed_projections<I, K>(&self, _keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
    }
}

struct NoopObserver;
impl KernelEventObserver for NoopObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

#[test]
fn register_observer_projection_calls_observer_then_projection() {
    let reg = FakeRegistrar::new();
    let observer = Arc::new(NoopObserver) as Arc<dyn KernelEventObserver>;

    let id = register_observer_projection(&reg, observer, "test.key", || None);

    assert!(id.is_some(), "expected a valid observer id");
    assert_ne!(id.unwrap().0, 0, "id should not be zero");

    let calls = reg.recorded();
    assert_eq!(
        calls,
        vec!["register_event_observer", "register_typed_snapshot_projection"],
        "register_event_observer must be called before register_typed_snapshot_projection"
    );
}

#[test]
fn register_observer_projection_zero_id_skips_projection() {
    // When register_event_observer returns id 0 (slot poisoned), the
    // projection must NOT be registered (D6 contract).
    let reg = FakeRegistrar::new_with_zero_id();
    let observer = Arc::new(NoopObserver) as Arc<dyn KernelEventObserver>;

    let id = register_observer_projection(&reg, observer, "test.key", || None);

    assert!(id.is_none(), "zero id must return None");
    let calls = reg.recorded();
    assert_eq!(
        calls,
        vec!["register_event_observer"],
        "projection must not be registered when observer id is 0"
    );
}
