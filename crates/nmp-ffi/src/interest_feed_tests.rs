//! `NmpApp::open_observed_projection` / `close_observed_projection` — observer
//! lifecycle tests (replaces the former `register_feed_with_observer` seam,
//! which was deleted in issue #2089).
//!
//! These exercise the `ObservedProjectionRegistrar` seam: an observer registered
//! via `open_observed_projection` receives a non-zero slot id (observer
//! acquired), and `close_observed_projection` tears it down cleanly without
//! panicking. The actor need not be started — the interest commands enqueued
//! during open/close are silently dropped when the channel has no receiver, and
//! the observer-slot side effects (allocate / revoke) happen synchronously.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionSink;

use crate::{nmp_app_free, nmp_app_new};

struct StubObserver {
    call_count: AtomicU32,
}

impl ObservedProjectionSink for StubObserver {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.call_count.fetch_add(1, Ordering::Relaxed);
    }
}

fn stub_decl(observer: Arc<dyn ObservedProjectionSink>, consumer: &str) -> ObservedProjection {
    ObservedProjection::from_kinds(observer, consumer, 0, [1], 16)
}

#[test]
fn open_observed_projection_returns_nonzero_id() {
    let app = nmp_app_new();
    {
        let app_ref = crate::app_ref(app).expect("app");
        let observer = Arc::new(StubObserver {
            call_count: AtomicU32::new(0),
        });
        let id = app_ref.open_observed_projection(stub_decl(
            observer as Arc<dyn ObservedProjectionSink>,
            "test.observed.basic",
        ));
        assert_ne!(
            id.0, 0,
            "open_observed_projection must allocate a non-zero observer id"
        );
        app_ref.close_observed_projection(id);
    }
    nmp_app_free(app);
}

#[test]
fn close_observed_projection_is_idempotent() {
    // A second close (e.g. a double-fired SwiftUI onDisappear) must not panic
    // and must silently no-op (D6): the sessions map has no entry for the id.
    let app = nmp_app_new();
    {
        let app_ref = crate::app_ref(app).expect("app");
        let observer = Arc::new(StubObserver {
            call_count: AtomicU32::new(0),
        });
        let id = app_ref.open_observed_projection(stub_decl(
            observer as Arc<dyn ObservedProjectionSink>,
            "test.observed.idempotent",
        ));
        app_ref.close_observed_projection(id);
        // Second close — session map has no entry; early-return, no panic.
        app_ref.close_observed_projection(id);
    }
    nmp_app_free(app);
}

#[test]
fn open_twice_allocates_distinct_ids() {
    // Two independent calls to open_observed_projection must each receive a
    // distinct, non-zero observer id so their lifetimes are independently
    // managed.
    let app = nmp_app_new();
    {
        let app_ref = crate::app_ref(app).expect("app");
        let obs1 = Arc::new(StubObserver {
            call_count: AtomicU32::new(0),
        });
        let obs2 = Arc::new(StubObserver {
            call_count: AtomicU32::new(0),
        });
        let id1 = app_ref.open_observed_projection(stub_decl(
            obs1 as Arc<dyn ObservedProjectionSink>,
            "test.observed.a",
        ));
        let id2 = app_ref.open_observed_projection(stub_decl(
            obs2 as Arc<dyn ObservedProjectionSink>,
            "test.observed.b",
        ));
        assert_ne!(id1.0, 0, "first id must be non-zero");
        assert_ne!(id2.0, 0, "second id must be non-zero");
        assert_ne!(id1, id2, "each open must produce a distinct id");
        app_ref.close_observed_projection(id1);
        app_ref.close_observed_projection(id2);
    }
    nmp_app_free(app);
}
