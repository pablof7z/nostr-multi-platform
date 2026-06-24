use std::sync::Mutex;

use nmp_ffi::{nmp_app_free, nmp_app_new, FeedOpenError, NmpApp};

use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ProjectionKey,
};

static SERIAL: Mutex<()> = Mutex::new(());

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";

fn set_app_active(app: *mut NmpApp, active: Option<&str>) {
    let handle = unsafe { &*app }.active_account_handle();
    *handle.lock().expect("active-account slot") = active.map(str::to_string);
}

fn home_params() -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey("nmp.feed.home".into()),
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
fn active_follows_rejects_unsupported_render_and_projection() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let mut flat = home_params();
    flat.render = FeedRender::Flat;
    let err = app_ref
        .open_feed(&flat, &compiler)
        .expect_err("home default does not honor flat render");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "active-user-follows-render"),
        "typed fail-closed error for unsupported home render, got {err:?}"
    );

    let mut custom_projection = home_params();
    custom_projection.projection = ProjectionKey("test.feed.home.alias".into());
    let err = app_ref
        .open_feed(&custom_projection, &compiler)
        .expect_err("home default does not honor custom projection keys");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "active-user-follows-projection"),
        "typed fail-closed error for unsupported home projection, got {err:?}"
    );

    assert_eq!(app_ref.live_feed_session_count(), 0, "no session leaked");
    nmp_app_free(app);
}
