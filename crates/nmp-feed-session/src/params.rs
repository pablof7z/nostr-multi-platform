use std::collections::BTreeSet;

use nmp_feed::FeedParams;

use crate::PrimaryKindError;

/// Validate a [`FeedParams`] declaration's primary kinds and return the compiled
/// acquisition kind set (primary kinds plus derived repost/delete acquisition).
///
/// The protocol-agnostic param model lives in `nmp-feed`; the feed-session
/// compiler owns the reusable runtime-independent seam that turns those params
/// into acquisition. That seam is allowed to name the canonical NIP-18 primary
/// kind validator without making platform runtimes depend on NIP-18 directly.
pub fn validate_feed_params(params: &FeedParams) -> Result<BTreeSet<u32>, PrimaryKindError> {
    nmp_nip18::validate_primary_kinds(params.primary_kinds.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_feed::{
        FeedAdmission, FeedItemProjection, FeedOrder, FeedScope, FeedShape, FeedWindowPolicy,
        ProjectionKey,
    };

    const KIND_DELETE: u32 = 5;

    #[test]
    fn validate_feed_params_accepts_primary_and_derives_repost_and_delete_acquisition() {
        assert_eq!(
            validate_feed_params(&sample_params(vec![1])),
            Ok(BTreeSet::from([1, 6, KIND_DELETE]))
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![20])),
            Ok(BTreeSet::from([20, 16, KIND_DELETE]))
        );
    }

    #[test]
    fn validate_feed_params_fails_closed_on_wrapper_delete_empty() {
        assert_eq!(
            validate_feed_params(&sample_params(vec![1, 6])),
            Err(PrimaryKindError::RepostWrapper { kind: 6 }),
            "kind 6 is derived acquisition, not primary"
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![1, KIND_DELETE])),
            Err(PrimaryKindError::DeleteKind),
            "kind 5 is derived suppression, not primary"
        );
        assert_eq!(
            validate_feed_params(&sample_params(vec![])),
            Err(PrimaryKindError::EmptyPrimaryKinds),
            "an open feed must declare at least one primary kind"
        );
    }

    fn sample_params(primary_kinds: Vec<u32>) -> FeedParams {
        FeedParams {
            primary_kinds,
            shape: FeedShape::Flat,
            source: FeedScope::ActiveUserFollows,
            admission: FeedAdmission::All,
            order: FeedOrder::NewestByFeedPosition,
            window: FeedWindowPolicy::default(),
            key: ProjectionKey::app_owned("app.feed.following").unwrap(),
            item_projection: FeedItemProjection::FeedRows,
        }
    }
}
