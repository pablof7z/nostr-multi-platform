//! M2 (ADR-0042 §5.1, V-112) — `NmpApp::register_feed_with_observer` /
//! `unregister_feed` teardown-seam tests.
//!
//! These exercise the transient-feed registration the Chirp author/thread
//! symbols drive: a feed registered as BOTH a `FeedController` (output) AND a
//! `KernelEventObserver` (ingest) under one key, torn down in full on close. The
//! contract is observable without the actor: the feed registry's
//! `load_older(key)` reaches a live controller, and `unregister_feed` reports
//! whether anything was removed (so a re-open / double-close cannot silently
//! leak or panic).

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::FeedController;

use crate::{nmp_app_free, nmp_app_new};

/// A feed double that is both a controller (its `load_older` returns a known
/// sentinel so registration is observable) and an event observer (so a single
/// `Arc` plugs into both registries, exactly as `FlatFeed` does).
struct StubFeed;

impl FeedController for StubFeed {
    fn load_older(&self) -> bool {
        // A live controller under this key returns `true`; once unregistered,
        // `load_older_feed` finds no controller and returns `false`.
        true
    }
}

impl KernelEventObserver for StubFeed {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

#[test]
fn register_feed_with_observer_then_unregister_tears_down_the_controller() {
    let app = nmp_app_new();
    {
        let app_ref = crate::app_ref(app).expect("app");
        let key = "nmp.feed.author.alice";

        let feed = Arc::new(StubFeed);
        let controller: Arc<dyn FeedController> = feed.clone();
        let observer: Arc<dyn KernelEventObserver> = feed;
        app_ref.register_feed_with_observer(key, controller, observer);

        // The controller is live: `load_older` reaches the StubFeed sentinel.
        assert!(
            app_ref.load_older_feed(key),
            "registered feed controller must be reachable"
        );

        // Teardown reports success and the controller is gone afterwards.
        assert!(
            app_ref.unregister_feed(key),
            "unregister_feed must report it removed something"
        );
        assert!(
            !app_ref.load_older_feed(key),
            "controller must be unreachable after unregister_feed"
        );

        // Idempotent: a second close (e.g. a double-fired SwiftUI onDisappear)
        // is a harmless no-op that reports `false`.
        assert!(
            !app_ref.unregister_feed(key),
            "second unregister_feed of an absent key reports false"
        );
    }
    nmp_app_free(app);
}

#[test]
fn reopen_replaces_the_controller_without_panicking() {
    // The subtle path: a re-open under the same key revokes the prior observer
    // (`interest_feed_observers` insert returns `Some(previous)`) and installs
    // the new one. There is no public observer count to assert, but the second
    // registration must not panic and the controller must remain reachable, and
    // a SINGLE unregister must still fully tear the (current) feed down.
    let app = nmp_app_new();
    {
        let app_ref = crate::app_ref(app).expect("app");
        let key = "nmp.feed.thread.root";

        let first = Arc::new(StubFeed);
        app_ref.register_feed_with_observer(
            key,
            first.clone() as Arc<dyn FeedController>,
            first as Arc<dyn KernelEventObserver>,
        );

        let second = Arc::new(StubFeed);
        app_ref.register_feed_with_observer(
            key,
            second.clone() as Arc<dyn FeedController>,
            second as Arc<dyn KernelEventObserver>,
        );

        assert!(
            app_ref.load_older_feed(key),
            "controller remains reachable after a re-open"
        );
        assert!(
            app_ref.unregister_feed(key),
            "one unregister tears down the re-opened feed"
        );
        assert!(!app_ref.load_older_feed(key));
    }
    nmp_app_free(app);
}
