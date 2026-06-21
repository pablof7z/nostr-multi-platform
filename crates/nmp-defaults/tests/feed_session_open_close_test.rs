//! #1740 step 2 — `NmpApp::open_feed` / `close_feed` over the REAL op-feed
//! composition (`compile_feed_params`).
//!
//! Proves the session wrapper composes over the EXISTING home-feed mechanics
//! (`register_op_feed_defaults`): an `ActiveUserFollows` open returns a handle
//! with a projection key + session id; the session produces the typed NOFS
//! sidecar via the existing engine; `close_feed(handle)` tears the controller +
//! projection + ingest observers down (proven released: the controller becomes
//! unreachable, the typed sidecar stops emitting, and the session registry no
//! longer reports the id); double close is a no-op; and an unsupported scope
//! fails closed with the typed error, registering nothing.

use std::sync::Mutex;

use nmp_ffi::{nmp_app_free, nmp_app_new, FeedOpenError, NmpApp};

use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedScope, FeedWindow, ProjectionKey, TagTerm,
};

// One live `NmpApp` at a time — same harness-contention guard the sibling
// op-feed integration tests use.
static SERIAL: Mutex<()> = Mutex::new(());

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";

fn set_app_active(app: *mut NmpApp, active: Option<&str>) {
    // SAFETY: tests pass a live pointer returned by `nmp_app_new`.
    let handle = unsafe { &*app }.active_account_handle();
    *handle.lock().expect("active-account slot") = active.map(str::to_string);
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

/// Adapter so `compile_feed_params` (a free fn) satisfies `open_feed`'s compiler.
fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_defaults::compile_feed_params(app, params, kinds)
}

#[test]
fn open_feed_active_follows_over_real_op_feed_then_close_tears_down() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    assert!(!app.is_null());
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    // ── open ──────────────────────────────────────────────────────────────
    let handle = app_ref
        .open_feed(&home_params(), &compiler)
        .expect("active-follows home opens over the real op-feed mechanics");
    assert_eq!(handle.projection_key, ProjectionKey("nmp.feed.home".into()));
    assert_ne!(handle.session_id.0, 0, "minted a real session id");
    assert!(app_ref.feed_session_is_open(&handle), "session live");
    assert_eq!(app_ref.live_feed_session_count(), 1);

    // The session produces its sidecar through the EXISTING engine: the typed
    // NOFS sidecar is emitted under the home key while the session is live.
    // (`load_older_feed` reflects pull *progress*, not registration — with no
    // published event store pre-start it returns false even for a live
    // controller — so the typed sidecar is the registration probe here.)
    let typed_present_before = app_ref
        .run_typed_snapshot_projections()
        .iter()
        .any(|p| p.key == "nmp.feed.home");
    assert!(
        typed_present_before,
        "typed NOFS sidecar emitted by the live session"
    );

    // ── close via the HANDLE ────────────────────────────────────────────────
    assert!(app_ref.close_feed(&handle), "close tears the session down");

    // Proof of release (not a flag flip):
    // 1. session entry removed from the registry.
    assert!(!app_ref.feed_session_is_open(&handle), "session removed");
    assert_eq!(app_ref.live_feed_session_count(), 0, "no live sessions");
    // 2. the typed sidecar projection is removed: the first post-close tick
    //    emits a one-shot `Cleared` row for the key (the registry's documented
    //    removal signal), and every subsequent tick emits nothing for the key.
    //    Either way no live `Updated`/`Replaced` payload remains — proof the
    //    projection was released, not flag-flipped.
    use nmp_core::WireProjectionState;
    let post_close = app_ref.run_typed_snapshot_projections();
    let live_home_after = post_close
        .iter()
        .any(|p| p.key == "nmp.feed.home" && p.state != WireProjectionState::Cleared);
    assert!(
        !live_home_after,
        "no live home sidecar payload after close (only the one-shot Cleared)"
    );
    // A SECOND tick: the Cleared row was drained once; nothing remains.
    let next_tick = app_ref.run_typed_snapshot_projections();
    assert!(
        !next_tick.iter().any(|p| p.key == "nmp.feed.home"),
        "no home sidecar row at all on the tick after the Cleared drain"
    );

    // ── idempotent double close ─────────────────────────────────────────────
    assert!(!app_ref.close_feed(&handle), "second close is a no-op");
    assert!(!app_ref.close_feed(&handle), "third close is a no-op");

    nmp_app_free(app);
}

#[test]
fn unsupported_scope_fails_closed_and_registers_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    // #1740 step 3: `RelaySet` (no framework resolver) and `CustomPerspectiveId`
    // (step 4) stay fail-closed. They must register NOTHING.
    let mut params = home_params();
    params.acquisition = FeedScope::RelaySet {
        relays: nmp_feed::RelaySetId("my-relays".into()),
    };
    params.projection = ProjectionKey("test.feed.relayset".into());

    let err = app_ref
        .open_feed(&params, &compiler)
        .expect_err("RelaySet has no framework resolver — must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "RelaySet"),
        "typed fail-closed error naming the scope, got {err:?}"
    );

    let mut custom = home_params();
    custom.acquisition = FeedScope::CustomPerspectiveId(nmp_feed::CustomPerspectiveId("x".into()));
    let err = app_ref
        .open_feed(&custom, &compiler)
        .expect_err("CustomPerspectiveId is deferred to step 4 — must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "CustomPerspectiveId"),
        "typed fail-closed error naming the scope, got {err:?}"
    );

    assert_eq!(app_ref.live_feed_session_count(), 0, "no session leaked");
    let any_sidecar = app_ref.run_typed_snapshot_projections().iter().any(|p| {
        p.key == "test.feed.relayset" || p.key == "nmp.feed.home"
    });
    assert!(!any_sidecar, "no sidecar registered for a fail-closed open");

    nmp_app_free(app);
}

#[test]
fn custom_admission_or_ranking_fails_closed_and_registers_nothing() {
    // #1740 step 3: `FeedAdmission::Custom` / `FeedRanking::Custom` (and the
    // unsupported `ChronologicalAsc`) name an app-registered perspective whose
    // registration mechanism lands in step 4. The compiler must FAIL CLOSED with
    // the typed error and register NOTHING — never silently open with default
    // behavior, which would render the feed wider/mis-ordered vs the declaration.
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let custom_id = nmp_feed::CustomPerspectiveId("engagement".into());

    // Custom ADMISSION on an otherwise-supported (Tag) acquisition → fail closed.
    let mut admission = home_params();
    admission.acquisition = FeedScope::Tag {
        term: TagTerm("nostr".into()),
    };
    admission.admission = FeedAdmission::Custom(custom_id.clone());
    admission.projection = ProjectionKey("test.feed.custom-admission".into());
    let err = app_ref
        .open_feed(&admission, &compiler)
        .expect_err("custom admission is not wired — must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "custom-admission"),
        "typed fail-closed error for custom admission, got {err:?}"
    );

    // Custom RANKING → fail closed.
    let mut ranking = home_params();
    ranking.acquisition = FeedScope::Tag {
        term: TagTerm("nostr".into()),
    };
    ranking.ranking = FeedRanking::Custom(custom_id);
    ranking.projection = ProjectionKey("test.feed.custom-ranking".into());
    let err = app_ref
        .open_feed(&ranking, &compiler)
        .expect_err("custom ranking is not wired — must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "custom-ranking"),
        "typed fail-closed error for custom ranking, got {err:?}"
    );

    // The unsupported chronological-ascending order also fails closed (the
    // engine sorts newest-first only).
    let mut asc = home_params();
    asc.acquisition = FeedScope::Tag {
        term: TagTerm("nostr".into()),
    };
    asc.ranking = FeedRanking::ChronologicalAsc;
    asc.projection = ProjectionKey("test.feed.asc".into());
    let err = app_ref
        .open_feed(&asc, &compiler)
        .expect_err("ChronologicalAsc is not wired — must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "custom-ranking"),
        "typed fail-closed error for ascending order, got {err:?}"
    );

    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "no session leaked for a fail-closed custom admission/ranking open"
    );

    nmp_app_free(app);
}

#[test]
fn tag_scope_opens_and_closes_over_session_engine() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    // #1740 step 3: `Tag` IS supported now — it compiles to a `#t` acquisition
    // with an EVENT-AWARE `#t` admission predicate and registers a session
    // engine under its own key.
    let mut params = home_params();
    params.acquisition = FeedScope::Tag {
        term: TagTerm("nostr".into()),
    };
    params.projection = ProjectionKey("test.feed.tag".into());

    let handle = app_ref
        .open_feed(&params, &compiler)
        .expect("Tag scope opens over the session engine");
    assert_eq!(handle.projection_key, ProjectionKey("test.feed.tag".into()));
    assert!(app_ref.feed_session_is_open(&handle), "session live");
    assert_eq!(app_ref.live_feed_session_count(), 1);

    // The session registered its own typed sidecar under the unique key.
    let present = app_ref
        .run_typed_snapshot_projections()
        .iter()
        .any(|p| p.key == "test.feed.tag");
    assert!(present, "session sidecar emitted under the unique key");

    // Close tears it down via the handle (symmetric: withdraws the #t interest,
    // removes the controller + projection, revokes the engine observer).
    assert!(app_ref.close_feed(&handle), "close tears the session down");
    assert!(!app_ref.feed_session_is_open(&handle));
    assert_eq!(app_ref.live_feed_session_count(), 0, "no live sessions");

    nmp_app_free(app);
}
