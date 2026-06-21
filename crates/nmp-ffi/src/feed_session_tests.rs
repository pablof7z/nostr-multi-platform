//! `NmpApp::open_feed` / `close_feed` proofs over the EXISTING feed mechanics
//! (#1740 step 2).
//!
//! The compiler used here drives the SAME primitives the home feed uses
//! (`register_feed_with_observer` + `register_typed_snapshot_projection` +
//! the event-observer registry), so these prove the session wrapper composes
//! over real registrations: open returns a handle with a projection key + id,
//! the registered controller is reachable, and `close_feed(handle)` tears down
//! the controller, the projection, and the observer — proven released, not
//! flag-flipped, by (a) the controller becoming unreachable, (b) the session
//! registry no longer reporting the id, and (c) the observer `Arc` strong count
//! dropping once the registry no longer holds it.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::{
    FeedAdmission, FeedController, FeedParams, FeedRanking, FeedScope, FeedSessionBuild, FeedWindow,
    ProjectionKey,
};

use crate::feed_session::{FeedCompileOutput, FeedOpenError};
use crate::{nmp_app_free, nmp_app_new, NmpApp};

/// A feed double registered as BOTH a controller (reachable `load_older`
/// sentinel) and an observer (one `Arc` plugs into both registries, like
/// `FlatFeed`). Counts observer fan-out so we can show the observer is gone.
struct StubFeed {
    observed: AtomicUsize,
}
impl StubFeed {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            observed: AtomicUsize::new(0),
        })
    }
}
impl FeedController for StubFeed {
    fn load_older(&self) -> bool {
        true
    }
}
impl KernelEventObserver for StubFeed {
    fn on_kernel_event(&self, _event: &KernelEvent) {
        self.observed.fetch_add(1, Ordering::SeqCst);
    }
}

fn home_params() -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey("nmp.feed.home".into()),
    }
}

/// A compiler that wires the active-follows home feed over the existing
/// mechanics and returns a teardown recipe that reuses the existing
/// `unregister_feed` path. Holds the feed `Arc` so the test can watch its
/// strong count fall after teardown drops the registry's references.
fn home_compiler(
    feed: Arc<StubFeed>,
) -> impl Fn(
    &NmpApp,
    &FeedParams,
    &std::collections::BTreeSet<u32>,
) -> Result<FeedCompileOutput, FeedOpenError> {
    move |app: &NmpApp, params: &FeedParams, _kinds: &std::collections::BTreeSet<u32>| {
        // Only the active-follows home scope is wired by the compiler used in
        // this FFI-level test; anything else fails closed (mirrors the real
        // `nmp-defaults` compiler's contract).
        if params.acquisition != FeedScope::ActiveUserFollows {
            return Err(FeedOpenError::ScopeNotSupportedYet {
                scope: "test-only-compiler",
            });
        }
        let key = params.projection.0.clone();
        // Register over the EXISTING mechanics, exactly as the home feed does:
        // a permanent controller (output) + an ingest observer (returns an id) +
        // a typed sidecar projection under the same key.
        let controller: Arc<dyn FeedController> = feed.clone();
        app.register_feed(key.clone(), controller);
        let observer: Arc<dyn KernelEventObserver> = feed.clone();
        let observer_id = app.register_event_observer(observer);
        app.register_typed_snapshot_projection(key.clone(), || None);

        // Teardown captures the registry SLOTS (not `&app`) via `FeedTeardown`
        // and reuses the same underlying unregister primitives — handle-based,
        // no re-derived filter. Registration order: controller, observer,
        // projection → teardown reverses to projection, observer, controller,
        // then a final mark-changed.
        let teardown = app.feed_teardown();
        Ok(FeedSessionBuild {
            projection_key: ProjectionKey(key.clone()),
            teardown: vec![
                teardown.unregister_feed(key.clone()),
                teardown.revoke_observer(observer_id),
                teardown.remove_projection(key),
                teardown.mark_changed(),
            ],
        })
    }
}

#[test]
fn open_feed_active_follows_returns_handle_with_key_and_session_id() {
    let app = nmp_app_new();
    {
        let app = crate::app_ref(app).expect("app");
        let feed = StubFeed::new();
        let params = home_params();
        let handle = app
            .open_feed(&params, &home_compiler(feed.clone()))
            .expect("active-follows home opens");

        assert_eq!(
            handle.projection_key,
            ProjectionKey("nmp.feed.home".into()),
            "handle carries the projection key"
        );
        assert_ne!(handle.session_id.0, 0, "minted a real session id");
        assert!(app.feed_session_is_open(&handle), "session is live");
        assert_eq!(app.live_feed_session_count(), 1);

        // The session produces rows/sidecar via the existing mechanics: the
        // registered controller is reachable.
        assert!(
            app.load_older_feed("nmp.feed.home"),
            "registered controller reachable through the existing feed registry"
        );
    }
    nmp_app_free(app);
}

#[test]
fn close_feed_tears_down_controller_projection_and_observer_no_leak() {
    let app = nmp_app_new();
    {
        let app = crate::app_ref(app).expect("app");
        let feed = StubFeed::new();
        let params = home_params();
        let handle = app
            .open_feed(&params, &home_compiler(feed.clone()))
            .expect("opens");

        // Before close: controller reachable, session live, and the feed `Arc`
        // is held by BOTH the controller and the observer registry (plus the
        // local `feed` binding) → strong count > 1.
        assert!(app.load_older_feed("nmp.feed.home"));
        let strong_before = Arc::strong_count(&feed);
        assert!(
            strong_before >= 3,
            "controller + observer registries hold the feed Arc (got {strong_before})"
        );

        // Close via the HANDLE (not a re-derived filter).
        assert!(app.close_feed(&handle), "close tears the session down");

        // Proof of release (not a flag flip):
        // 1. the session entry is GONE from the registry.
        assert!(!app.feed_session_is_open(&handle), "session removed");
        assert_eq!(app.live_feed_session_count(), 0, "no live sessions — no leak");
        // 2. the controller is unreachable (registry dropped it).
        assert!(
            !app.load_older_feed("nmp.feed.home"),
            "controller unreachable after close"
        );
        // 3. the observer registry released its `Arc` clone — strong count fell.
        let strong_after = Arc::strong_count(&feed);
        assert!(
            strong_after < strong_before,
            "feed Arc released by the registries on teardown ({strong_before} -> {strong_after})"
        );
        assert_eq!(
            strong_after, 1,
            "only the local test binding holds the feed Arc after teardown"
        );
    }
    nmp_app_free(app);
}

#[test]
fn close_feed_is_idempotent_double_close_is_a_noop() {
    let app = nmp_app_new();
    {
        let app = crate::app_ref(app).expect("app");
        let feed = StubFeed::new();
        let handle = app
            .open_feed(&home_params(), &home_compiler(feed))
            .expect("opens");

        assert!(app.close_feed(&handle), "first close tears down");
        // Second + third close: no panic, report false, teardown does not rerun
        // (the controller is already gone — re-running unregister would still be
        // safe, but the session entry is removed so teardown never fires again).
        assert!(!app.close_feed(&handle), "second close is a no-op");
        assert!(!app.close_feed(&handle), "third close is a no-op");
        assert_eq!(app.live_feed_session_count(), 0);
    }
    nmp_app_free(app);
}

#[test]
fn unsupported_scope_fails_closed_with_typed_error_and_registers_nothing() {
    let app = nmp_app_new();
    {
        let app = crate::app_ref(app).expect("app");
        let feed = StubFeed::new();
        // A scope the compiler does not wire (e.g. a hashtag firehose).
        let mut params = home_params();
        params.acquisition = FeedScope::Tag {
            term: nmp_feed::TagTerm("nostr".into()),
        };

        let err = app
            .open_feed(&params, &home_compiler(feed))
            .expect_err("unsupported scope must fail closed");
        assert!(
            matches!(err, FeedOpenError::ScopeNotSupportedYet { .. }),
            "typed fail-closed error, got {err:?}"
        );
        // Nothing was registered and no session minted.
        assert_eq!(app.live_feed_session_count(), 0, "no session leaked");
        assert!(
            !app.load_older_feed("nmp.feed.home"),
            "no controller registered for a fail-closed open"
        );
    }
    nmp_app_free(app);
}

#[test]
fn invalid_primary_kinds_fail_closed_before_the_compiler_runs() {
    let app = nmp_app_new();
    {
        let app = crate::app_ref(app).expect("app");
        let ran = Arc::new(AtomicUsize::new(0));
        let ran_c = Arc::clone(&ran);
        let compiler = move |_app: &NmpApp,
                             _p: &FeedParams,
                             _k: &std::collections::BTreeSet<u32>|
              -> Result<FeedCompileOutput, FeedOpenError> {
            ran_c.fetch_add(1, Ordering::SeqCst);
            unreachable!("compiler must not run for invalid primary kinds");
        };
        let mut params = home_params();
        params.primary_kinds = vec![1, 6]; // kind 6 is a repost wrapper → reject

        let err = app.open_feed(&params, &compiler).expect_err("rejected");
        assert!(
            matches!(err, FeedOpenError::InvalidParams(_)),
            "typed invalid-params error, got {err:?}"
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0, "compiler never ran");
        assert_eq!(app.live_feed_session_count(), 0);
    }
    nmp_app_free(app);
}
