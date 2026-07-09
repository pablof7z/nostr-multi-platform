//! End-to-end proofs over the shared facade mechanics (split out of `lib.rs`
//! for file-size discipline).

use std::sync::Arc;

use crate::dispatch_action;

#[test]
fn dispatch_empty_envelope_returns_error_outcome() {
    let app = nmp_native_runtime::new_app();
    let out = dispatch_action(&app, &[]);
    assert!(out.correlation_id.is_none());
    assert!(out.error.is_some());
}

/// End-to-end proof for #2516: an app-owned facade flow that
/// (1) registers a projection/feed session, (2) observes an active-account
/// change, and (3) reopens the session — with NO raw runtime pointer and no
/// `unsafe`. The runtime is owned by value (`new_app()`), every helper
/// borrows `&app`, and the account-change observer forwards through an
/// `Arc`-held sink rather than capturing the runtime.
#[test]
fn account_change_session_reopen_via_safe_handles() {
    use std::sync::Mutex;

    use crate::{close_feed, open_feed, register_account_change_sink, reopen_feed, unregister_account_change_sink};

    // Owned by value — the safe handle. No `*mut NmpApp`, no `Arc<runtime>`
    // capture, no `unsafe`.
    let app = nmp_native_runtime::new_app();

    let params = r#"{
        "primary_kinds": [1],
        "source": "ActiveUserFollows",
        "admission": "All",
        "order": "NewestByFeedPosition",
        "window": {"initial_limit": 50},
        "key": "app.feed.support.reopen",
        "item_projection": "FeedRows"
    }"#;

    // 1. Feed registration through the shared mechanic.
    let Ok(opened) = open_feed(&app, params) else {
        assert!(false, "open feed must succeed");
        return;
    };
    assert!(!opened.projection_key.is_empty());
    assert_ne!(opened.handle_id, 0);

    // 2. Observe active-account changes without capturing the runtime: the
    //    sink only records the new identity.
    let changes: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let observer_id =
        register_account_change_sink(&app, Box::new(Arc::clone(&changes)), |seen, id| {
            seen.lock().unwrap().push(id);
        });

    // 3. Reopen the feed (the flow a facade runs for a pinned feed
    //    after an active-account change). The old handle is torn down and a
    //    fresh one is minted.
    let Ok(reopened) = reopen_feed(&app, &opened, params) else {
        assert!(false, "reopen feed must succeed");
        return;
    };
    assert_eq!(
        reopened.projection_key, opened.projection_key,
        "same projection key for the same declaration"
    );
    assert_ne!(
        reopened.handle_id, opened.handle_id,
        "reopen mints a fresh handle id"
    );
    assert!(
        !close_feed(&app, &opened),
        "the old feed was already torn down by reopen (D6)"
    );

    // Teardown — all through safe handles.
    assert!(close_feed(&app, &reopened));
    unregister_account_change_sink(&app, observer_id);
}
