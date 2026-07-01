//! Engine-agnostic proofs for the [`FeedSessionRegistry`] (#1740 step 2).
//!
//! These exercise the registry mechanics in isolation — no OP-feed engine, no
//! `NmpApp` — so they prove the *registry* contract: it records a teardown
//! recipe, runs it exactly once on close (in reverse order), is idempotent on
//! double close, and frees the map entry (proving teardown releases rather than
//! flipping a flag). The wired-over-real-mechanics proofs live in `explicit composition`
//! / `nmp-ffi`.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::*;
use crate::params::ProjectionKey;

fn build_with<F: FnOnce() + Send + 'static>(key: &str, teardown: F) -> FeedSessionBuild {
    FeedSessionBuild {
        projection_key: ProjectionKey::app_owned(key).unwrap(),
        teardown: vec![Box::new(teardown)],
    }
}

#[test]
fn open_mints_distinct_ids_and_records_projection_key() {
    let reg = FeedSessionRegistry::default();
    let a = reg.open(build_with("test.feed.following", || {}));
    let b = reg.open(build_with("app.feed.author.alice", || {}));
    assert_ne!(a, b, "each open mints a distinct id");
    assert_ne!(
        a,
        FeedSessionId(0),
        "minted id is never the reserved sentinel"
    );
    assert_eq!(
        reg.projection_key(&a),
        Some(ProjectionKey::app_owned("test.feed.following").unwrap())
    );
    assert_eq!(reg.live_count(), 2, "two live sessions");
}

#[test]
fn close_runs_teardown_exactly_once_and_frees_the_entry() {
    let reg = FeedSessionRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_t = Arc::clone(&calls);
    let id = reg.open(build_with("test.feed.following", move || {
        calls_t.fetch_add(1, Ordering::SeqCst);
    }));

    assert!(reg.is_open(&id), "session is live before close");
    assert!(reg.close(&id), "close reports the session was present");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "teardown ran exactly once");
    // Proof of release (not a flag flip): the entry is GONE from the map, so the
    // captured closure (and anything it held) has been dropped.
    assert!(!reg.is_open(&id), "session entry is removed after close");
    assert_eq!(reg.live_count(), 0, "no live sessions remain — no leak");
    assert_eq!(reg.projection_key(&id), None, "key no longer resolvable");
}

#[test]
fn double_close_is_idempotent_no_panic_no_second_teardown() {
    let reg = FeedSessionRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_t = Arc::clone(&calls);
    let id = reg.open(build_with("test.feed.following", move || {
        calls_t.fetch_add(1, Ordering::SeqCst);
    }));

    assert!(reg.close(&id), "first close tears down");
    // Second close: no panic, returns false, teardown does NOT run again.
    assert!(!reg.close(&id), "second close is a no-op");
    assert!(!reg.close(&id), "third close is still a no-op");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "teardown never runs more than once across repeated closes"
    );
}

#[test]
fn close_unknown_id_is_a_noop() {
    let reg = FeedSessionRegistry::default();
    assert!(
        !reg.close(&FeedSessionId(999)),
        "closing an id that was never opened is a harmless no-op"
    );
}

#[test]
fn teardown_runs_in_reverse_registration_order() {
    let reg = FeedSessionRegistry::default();
    let order = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let (o1, o2, o3) = (Arc::clone(&order), Arc::clone(&order), Arc::clone(&order));
    let build = FeedSessionBuild {
        projection_key: ProjectionKey::app_owned("test.feed.following").unwrap(),
        teardown: vec![
            Box::new(move || o1.lock().unwrap().push(1)),
            Box::new(move || o2.lock().unwrap().push(2)),
            Box::new(move || o3.lock().unwrap().push(3)),
        ],
    };
    let id = reg.open(build);
    reg.close(&id);
    assert_eq!(
        *order.lock().unwrap(),
        vec![3, 2, 1],
        "last-registered teardown runs first (reverse-release discipline)"
    );
}

#[test]
fn dropping_the_registry_drops_unclosed_session_closures() {
    // A session that is never explicitly closed must still have its captured
    // resources released when the registry itself is dropped — no leak.
    let live = Arc::new(AtomicUsize::new(0));
    {
        let reg = FeedSessionRegistry::default();
        let guard = Arc::clone(&live);
        guard.fetch_add(1, Ordering::SeqCst);
        // The teardown closure holds `guard`; when the registry drops, the
        // closure (and its Arc clone) drop too. We assert via Arc strong count.
        let _id = reg.open(FeedSessionBuild {
            projection_key: ProjectionKey::app_owned("test.feed.following").unwrap(),
            teardown: vec![Box::new(move || {
                // Never invoked (registry dropped without close), but `guard` is
                // captured and must drop with the registry.
                let _ = &guard;
            })],
        });
        assert_eq!(
            Arc::strong_count(&live),
            2,
            "registry holds the closure that captured the second Arc"
        );
    } // reg dropped here
    assert_eq!(
        Arc::strong_count(&live),
        1,
        "dropping the registry dropped the unclosed session's captured Arc — no leak"
    );
}
