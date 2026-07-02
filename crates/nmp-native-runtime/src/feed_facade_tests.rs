use crate::{
    new_app, FeedAdmission, FeedHandle, FeedOrder, FeedParams, FeedScope, FeedShape,
    FeedWindowPolicy, ProjectionKey,
};

fn active_follows_params(key: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        shape: FeedShape::RootIndexed,
        source: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        order: FeedOrder::NewestByFeedPosition,
        window: FeedWindowPolicy { initial_limit: 80 },
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
    assert!(
        !app.feeds().close(&forged),
        "mismatched handle must not close the live feed"
    );
    assert!(app.feed_session_is_open(&handle));

    assert!(app.feeds().close(&handle));
}
