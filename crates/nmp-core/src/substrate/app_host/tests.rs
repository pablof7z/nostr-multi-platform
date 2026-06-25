use super::*;
use crate::substrate::KernelEvent;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Fake registrar that records which calls were made and in what order.
/// Also stores the last observer passed to `register_live_event_tap` so tests
/// can fire it and verify the observer itself was invoked.
struct FakeRegistrar {
    calls: RefCell<Vec<&'static str>>,
    next_id: RefCell<u64>,
    last_observer: RefCell<Option<Arc<dyn KernelEventObserver>>>,
}

impl FakeRegistrar {
    fn new() -> Self {
        FakeRegistrar {
            calls: RefCell::new(Vec::new()),
            next_id: RefCell::new(1),
            last_observer: RefCell::new(None),
        }
    }

    fn new_with_zero_id() -> Self {
        FakeRegistrar {
            calls: RefCell::new(Vec::new()),
            next_id: RefCell::new(0),
            last_observer: RefCell::new(None),
        }
    }

    fn recorded(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }

    /// Fire the stored observer with a dummy event, returning whether an
    /// observer was present to fire.
    fn fire_last_observer(&self, event: &KernelEvent) -> bool {
        if let Some(obs) = self.last_observer.borrow().as_ref() {
            obs.on_kernel_event(event);
            true
        } else {
            false
        }
    }
}

impl LiveEventTapRegistrar for FakeRegistrar {
    fn register_live_event_tap(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        let id = *self.next_id.borrow();
        *self.next_id.borrow_mut() = id + 1;
        self.calls.borrow_mut().push("register_live_event_tap");
        *self.last_observer.borrow_mut() = Some(observer);
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

/// Observer that sets a shared flag when `on_kernel_event` is called.
struct FlagObserver {
    fired: Arc<AtomicBool>,
}
impl KernelEventObserver for FlagObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.fired.store(true, Ordering::SeqCst);
    }
}

fn dummy_kernel_event() -> KernelEvent {
    KernelEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        author: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        kind: 1,
        created_at: 0,
        tags: vec![],
        content: String::new(),
        relay_provenance: vec![],
    }
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
        vec!["register_live_event_tap", "register_typed_snapshot_projection"],
        "register_live_event_tap must be called before register_typed_snapshot_projection"
    );
}

#[test]
fn register_observer_projection_zero_id_skips_projection() {
    // When register_live_event_tap returns id 0 (slot poisoned), the
    // projection must NOT be registered (D6 contract) — but the observer
    // itself must still be callable (it was handed to the registrar and fires
    // when the registrar dispatches events to it).
    let reg = FakeRegistrar::new_with_zero_id();
    let fired = Arc::new(AtomicBool::new(false));
    let observer = Arc::new(FlagObserver {
        fired: Arc::clone(&fired),
    }) as Arc<dyn KernelEventObserver>;

    let id = register_observer_projection(&reg, observer, "test.key", || None);

    assert!(id.is_none(), "zero id must return None");
    let calls = reg.recorded();
    assert_eq!(
        calls,
        vec!["register_live_event_tap"],
        "projection must not be registered when observer id is 0"
    );

    // The observer was passed to the registrar and must still fire when the
    // registrar dispatches a kernel event to it — proving the observer
    // itself is live even though the projection step was skipped.
    let did_fire = reg.fire_last_observer(&dummy_kernel_event());
    assert!(did_fire, "registrar must have received and stored the observer");
    assert!(
        fired.load(Ordering::SeqCst),
        "observer must fire when the registrar dispatches a kernel event to it"
    );
}
