//! NIP-22 comment runtime wiring.
//!
//! Installs one shared [`CommentThreadProjection`] as the kind:1111 observer
//! and registers the `nmp.nip22.post_comment` action. Mirrors
//! [`super::bookmarks_runtime`] — observer + action wired against the same
//! crate-owned read model.

use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionSink;
use nmp_nip22::{CommentThreadProjection, KIND_COMMENT};

/// Wire the kind:1111 comment-thread projection and the post-comment action.
///
/// Returns the shared `Arc<CommentThreadProjection>` so an app that renders
/// comment threads can snapshot it directly; callers that only need the
/// publish path may drop it.
pub fn register_comment_runtime(
    app: &mut (impl ActionRegistrar + ObservedProjectionRegistrar),
) -> Arc<CommentThreadProjection> {
    let projection = Arc::new(CommentThreadProjection::new());

    app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        "nmp.nip22.comments",
        1,
        [KIND_COMMENT],
        512,
    ));

    nmp_nip22::register_actions(app);
    projection
}
