use std::sync::Mutex;

use nmp_ffi::{nmp_app_free, nmp_app_new, NmpApp};

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
) -> Result<nmp_feed::FeedSessionBuild, nmp_ffi::FeedOpenError> {
    nmp_defaults::compile_feed_params(app, params, kinds)
}

#[test]
fn active_follows_accepts_generic_render_and_projection() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_app_new();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let mut flat = home_params();
    flat.render = FeedRender::Flat;
    let _flat_handle = app_ref
        .open_feed(&flat, &compiler)
        .expect("active follows is a normal reduced-source feed");

    let mut custom_projection = home_params();
    custom_projection.projection = ProjectionKey("test.feed.home.alias".into());
    let _custom_handle = app_ref
        .open_feed(&custom_projection, &compiler)
        .expect("active follows supports custom projection keys");

    assert_eq!(
        app_ref.live_feed_session_count(),
        2,
        "sessions stay owned by the generic feed path"
    );
    nmp_app_free(app);
}
