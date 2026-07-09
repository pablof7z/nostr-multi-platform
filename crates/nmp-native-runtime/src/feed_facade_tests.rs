use crate::{
    new_app, CustomAdmissionDef, CustomAdmissionId, CustomOrderDef, CustomOrderId, CustomSourceDef,
    CustomSourceId, FeedAdmission, FeedHandle, FeedKey, FeedLoadStopReason, FeedOrder, FeedParams,
    FeedScope, FeedShape, FeedSpecOpenError, FeedWindowPolicy, ProjectionKey,
};

fn active_follows_params(key: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        shape: FeedShape::Flat,
        source: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        order: FeedOrder::NewestByFeedPosition,
        window: FeedWindowPolicy::bounded(80),
        key: ProjectionKey::app_owned(key).unwrap(),
        item_projection: nmp_feed::FeedItemProjection::FeedRows,
    }
}

#[test]
fn feeds_facade_opens_and_closes_through_canonical_lifecycle() {
    let app = new_app();
    let params = active_follows_params("test.feed.facade");

    let handle = app
        .feeds()
        .open(&params)
        .expect("canonical feed compiler opens active follows");

    assert_eq!(handle.projection_key, params.key);
    assert!(app.feed_session_is_open(&handle));
    assert_eq!(app.live_feed_session_count(), 1);

    assert!(app.feeds().close(&handle));
    assert!(!app.feed_session_is_open(&handle));
    assert_eq!(app.live_feed_session_count(), 0);
}

#[test]
fn feeds_facade_opens_spec_through_canonical_lifecycle() {
    let app = new_app();
    let key = FeedKey::app("test.feed.facade.spec").unwrap();
    let spec = nmp_feed::feed::events()
        .primary_kinds([1])
        .from(nmp_feed::source::active_user().follows())
        .shape(FeedShape::Flat)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(80))
        .project(nmp_feed::FeedItemProjection::feed_rows());

    let handle = app
        .feeds()
        .open_spec(key.clone(), spec)
        .expect("canonical feed compiler opens feed spec");

    assert_eq!(handle.projection_key, key);
    assert!(app.feed_session_is_open(&handle));
    assert!(app.feeds().close(&handle));
}

#[test]
fn feeds_facade_rejects_incomplete_spec_without_opening_session() {
    let app = new_app();
    let key = FeedKey::app("test.feed.facade.spec.invalid").unwrap();
    let err = app
        .feeds()
        .open_spec(key, nmp_feed::feed::events().primary_kinds([1]))
        .expect_err("missing source fails before opening");

    assert_eq!(
        err,
        FeedSpecOpenError::InvalidSpec(nmp_feed::FeedSpecError::MissingSource)
    );
    assert_eq!(app.live_feed_session_count(), 0);
}

#[test]
fn feeds_facade_pages_and_closes_only_matching_handles() {
    let app = new_app();
    let params = active_follows_params("test.feed.facade.handle");
    let handle = app.feeds().open(&params).expect("opens");
    let forged = FeedHandle {
        projection_key: ProjectionKey::app_owned("test.feed.facade.other").unwrap(),
        session_id: handle.session_id.clone(),
    };

    assert!(
        !app.feeds().load_older(&forged),
        "mismatched handle must not page the live feed"
    );
    let forged_status = app.feeds().load_older_status(&forged);
    assert!(!forged_status.changed);
    assert_eq!(forged_status.reason, FeedLoadStopReason::SessionUnavailable);
    let status = app.feeds().load_older_status(&handle);
    assert!(!status.changed);
    assert_eq!(
        status.reason,
        FeedLoadStopReason::SourceUnavailable,
        "no active account means active-follows source fails closed"
    );
    assert!(
        !app.feeds().close(&forged),
        "mismatched handle must not close the live feed"
    );
    assert!(app.feed_session_is_open(&handle));

    assert!(app.feeds().close(&handle));
}

#[test]
fn custom_feed_policy_registry_is_phase_specific() {
    let app = new_app();
    let source_id = CustomSourceId("test.source".into());
    let admission_id = CustomAdmissionId("test.admission".into());
    let order_id = CustomOrderId("test.order".into());

    assert!(app.register_custom_source(
        source_id.clone(),
        CustomSourceDef::new(FeedScope::Tag {
            term: nmp_feed::TagTerm("rust".into()),
        }),
    ));
    assert!(app.register_custom_admission(
        admission_id.clone(),
        CustomAdmissionDef::new(FeedScope::Tag {
            term: nmp_feed::TagTerm("safe".into()),
        }),
    ));
    assert!(app.register_custom_order(
        order_id.clone(),
        CustomOrderDef::new(FeedOrder::NewestByFeedPosition),
    ));

    assert!(app.custom_source(&source_id).is_some());
    assert!(app.custom_admission(&admission_id).is_some());
    assert!(app.custom_order(&order_id).is_some());
    assert_eq!(app.custom_feed_policy_count(), 3);
    assert!(
        !app.register_custom_source(
            source_id,
            CustomSourceDef::new(FeedScope::ActiveUserFollows),
        ),
        "custom policy ids are immutable once registered"
    );
}
