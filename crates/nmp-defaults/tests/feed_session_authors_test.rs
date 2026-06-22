//! #1740 read lane — the `FeedScope::Authors` static-author-set scope over the
//! REAL op-feed composition (`compile_feed_params` via `NmpApp::open_feed`).
//!
//! Proves the new closed-algebra variant opens over the session engine (an
//! `AdmitExpr::Authors` admission gate + a fixed author+kind acquisition) and
//! tears down idempotently by HANDLE, and that an EMPTY author set fails CLOSED
//! (never silently opens a feed that admits everyone) registering nothing. The
//! per-author ADMISSION proof (only the target author's events are admitted, a
//! non-author's are excluded) lives as a pure unit test in
//! `session_compile::resolve_tests`.

use std::sync::Mutex;

use nmp_ffi::{nmp_app_free, nmp_app_new, FeedOpenError, NmpApp};

use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedScope, FeedWindow, ProjectionKey,
};

// One live `NmpApp` at a time — the harness-contention guard the sibling
// op-feed integration tests use.
static SERIAL: Mutex<()> = Mutex::new(());

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";

fn set_app_active(app: *mut NmpApp, active: Option<&str>) {
    // SAFETY: tests pass a live pointer returned by `nmp_app_new`.
    let handle = unsafe { &*app }.active_account_handle();
    *handle.lock().expect("active-account slot") = active.map(str::to_string);
}

fn author_params(authors: &[&str], projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        acquisition: FeedScope::Authors {
            authors: authors.iter().map(|s| (*s).to_string()).collect(),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
    }
}

fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_defaults::compile_feed_params(app, params, kinds)
}

#[test]
fn authors_scope_opens_and_closes_over_session_engine() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let params = author_params(&[BOB], "test.feed.author");
    let handle = app_ref
        .open_feed(&params, &compiler)
        .expect("Authors scope opens over the session engine");
    assert_eq!(
        handle.projection_key,
        ProjectionKey("test.feed.author".into())
    );
    assert!(app_ref.feed_session_is_open(&handle), "session live");
    assert_eq!(app_ref.live_feed_session_count(), 1);
    assert!(
        app_ref
            .run_typed_snapshot_projections()
            .iter()
            .any(|p| p.key == "test.feed.author"),
        "session sidecar emitted under the unique key"
    );

    // Handle-close tears it down (withdraws the author interest, removes the
    // controller + projection, revokes the engine observer) — idempotently.
    assert!(app_ref.close_feed(&handle), "close tears the session down");
    assert!(!app_ref.feed_session_is_open(&handle));
    assert_eq!(app_ref.live_feed_session_count(), 0, "no live sessions");
    assert!(!app_ref.close_feed(&handle), "second close is a no-op");

    nmp_app_free(app);
}

#[test]
fn empty_authors_scope_fails_closed_and_registers_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    // An EMPTY author set must fail closed — never silently open a feed that
    // admits everyone — and register nothing (no leak).
    let params = author_params(&[], "test.feed.author-empty");
    let err = app_ref
        .open_feed(&params, &compiler)
        .expect_err("an empty author set must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "Authors-empty-set"),
        "typed fail-closed error naming the empty author set, got {err:?}"
    );
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "no session leaked for a fail-closed empty-author open"
    );
    assert!(
        !app_ref
            .run_typed_snapshot_projections()
            .iter()
            .any(|p| p.key == "test.feed.author-empty"),
        "no sidecar registered for a fail-closed open"
    );

    nmp_app_free(app);
}
