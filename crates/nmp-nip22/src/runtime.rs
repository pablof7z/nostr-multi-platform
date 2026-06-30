//! Host-side NIP-22 comment runtime installer.
//!
//! Installs one shared [`CommentThreadProjection`] as the kind:1111 observer.
//! App-facing reply writes are registered through `nmp-replies`, which owns the
//! NIP-10-vs-NIP-22 policy.

use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionSink;

use crate::{CommentThreadProjection, KIND_NIP22_COMMENT};

/// Wire the kind:1111 comment-thread projection into `app`.
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
        [KIND_NIP22_COMMENT],
        512,
    ));

    projection
}
