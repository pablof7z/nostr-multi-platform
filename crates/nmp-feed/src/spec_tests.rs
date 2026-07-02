use super::*;
use crate::{DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT};

#[test]
fn feed_spec_builds_canonical_params() {
    let key = FeedKey::app("test.feed.builder").unwrap();
    let spec = feed::events()
        .primary_kinds([1])
        .from(source::active_user().follows())
        .shape(FeedShape::Flat)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(80))
        .project(FeedItemProjection::feed_rows());

    let params = spec.into_params(key.clone()).expect("valid feed spec");

    assert_eq!(params.key, key);
    assert_eq!(params.primary_kinds, vec![1]);
    assert_eq!(params.source, FeedSourceExpr::ActiveUserFollows);
    assert_eq!(params.shape, FeedShape::Flat);
    assert_eq!(params.window.initial_visible_limit(), 80);
    assert_eq!(params.window.page_size(), 80);
    assert_eq!(params.window.source_page_size(), 80);
    assert_eq!(params.item_projection, FeedItemProjection::FeedRows);
}

#[test]
fn feed_spec_requires_primary_kinds_and_source() {
    let key = FeedKey::app("test.feed.builder.required").unwrap();

    assert_eq!(
        feed::events()
            .from(source::active_user().follows())
            .into_params(key.clone()),
        Err(FeedSpecError::MissingPrimaryKinds)
    );
    assert_eq!(
        feed::events().primary_kinds([1]).into_params(key),
        Err(FeedSpecError::MissingSource)
    );
}

#[test]
fn feed_key_app_is_the_projection_key_model() {
    let key = FeedKey::app("test.feed.key").unwrap();
    assert_eq!(key.as_str(), "test.feed.key");
    assert!(FeedKey::app("nmp.feed.reserved").is_err());
    assert!(FeedKey::app("").is_err());
}

#[test]
fn window_bounded_constructor_uses_existing_clamp_rules() {
    assert_eq!(
        FeedWindowPolicy::bounded(0).bounded_limit(),
        DEFAULT_FEED_WINDOW_LIMIT
    );
    assert_eq!(
        FeedWindowPolicy::bounded(MAX_FEED_WINDOW_LIMIT + 1).bounded_limit(),
        MAX_FEED_WINDOW_LIMIT
    );
    assert_eq!(FeedWindowPolicy::bounded(25).bounded_limit(), 25);
}

#[test]
fn source_helpers_build_closed_source_expressions() {
    let static_authors = source::authors(["b".to_string(), "a".to_string(), "a".to_string()]);
    assert_eq!(
        static_authors,
        FeedSourceExpr::Authors {
            authors: ["a".to_string(), "b".to_string()].into_iter().collect(),
        }
    );
    assert_eq!(
        source::active_user().hosted_groups(),
        FeedSourceExpr::ActiveUserHostedGroups
    );
    assert_eq!(
        source::list_members("mutuals"),
        FeedSourceExpr::ListMembers {
            list: ListId("mutuals".into()),
        }
    );
    assert_eq!(
        source::pointer_target_hydration(source::active_user().follows(), [7, 1111]),
        FeedSourceExpr::PointerTargetHydration {
            pointers: Box::new(FeedSourceExpr::ActiveUserFollows),
            pointer_kinds: vec![7, 1111],
        }
    );
    assert_eq!(
        source::custom("for-you"),
        FeedSourceExpr::CustomSource(CustomSourceId("for-you".into())),
    );
}
