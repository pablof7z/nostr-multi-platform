//! Concept-neutral proofs for the [`ReadSessionRegistry`] (#2777 step 1).
//!
//! These exercise the registry mechanics in isolation — no host, no concept —
//! so they prove the *registry* contract: it records a teardown recipe, runs it
//! exactly once on close (in reverse order), is idempotent on double close, and
//! frees the map entry (proving teardown releases rather than flipping a flag).

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::*;

fn build_with<F: FnOnce() + Send + 'static>(key: &str, teardown: F) -> ReadSessionBuild {
    ReadSessionBuild {
        projection_key: key.to_string(),
        teardown: vec![Box::new(teardown)],
    }
}

#[test]
fn open_mints_distinct_ids_and_records_projection_key() {
    let reg = ReadSessionRegistry::default();
    let a = reg.open(build_with("app.feed.following", || {}));
    let b = reg.open(build_with("nmp.replies.summary.abc", || {}));
    assert_ne!(a, b, "each open mints a distinct id");
    assert_ne!(a, ReadSessionId(0), "minted id is never the reserved sentinel");
    assert_eq!(
        reg.projection_key(&a),
        Some("app.feed.following".to_string())
    );
    assert_eq!(reg.live_count(), 2, "two live sessions, one leak audit");
}

#[test]
fn close_runs_teardown_exactly_once_and_frees_the_entry() {
    let reg = ReadSessionRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_t = Arc::clone(&calls);
    let id = reg.open(build_with("nmp.replies.summary.abc", move || {
        calls_t.fetch_add(1, Ordering::SeqCst);
    }));

    assert!(reg.is_open(&id), "session is live before close");
    assert!(reg.close(&id), "close reports the session was present");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "teardown ran exactly once");
    assert!(!reg.is_open(&id), "session entry is removed after close");
    assert_eq!(reg.live_count(), 0, "no live sessions remain — no leak");
    assert_eq!(reg.projection_key(&id), None, "key no longer resolvable");
}

#[test]
fn double_close_is_idempotent_no_panic_no_second_teardown() {
    let reg = ReadSessionRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_t = Arc::clone(&calls);
    let id = reg.open(build_with("app.feed.following", move || {
        calls_t.fetch_add(1, Ordering::SeqCst);
    }));

    assert!(reg.close(&id), "first close tears down");
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
    let reg = ReadSessionRegistry::default();
    assert!(
        !reg.close(&ReadSessionId(999)),
        "closing an id that was never opened is a harmless no-op"
    );
}

#[test]
fn teardown_runs_in_reverse_registration_order() {
    let reg = ReadSessionRegistry::default();
    let order = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let (o1, o2, o3) = (Arc::clone(&order), Arc::clone(&order), Arc::clone(&order));
    let build = ReadSessionBuild {
        projection_key: "app.feed.following".to_string(),
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
    let live = Arc::new(AtomicUsize::new(0));
    {
        let reg = ReadSessionRegistry::default();
        let guard = Arc::clone(&live);
        guard.fetch_add(1, Ordering::SeqCst);
        let _id = reg.open(ReadSessionBuild {
            projection_key: "app.feed.following".to_string(),
            teardown: vec![Box::new(move || {
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
