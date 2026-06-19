//! ADR-0061 — bind the OP-centric home feed into the canonical feed surface.
//!
//! Split out of `op_feed_defaults.rs` for the 500-LOC file-size ceiling. This is
//! the single place the `{1,6}` (or any host-declared) contact-feed kinds enter
//! the surface: they move OUT of the shell / app-named ABI and INTO the Rust
//! `FeedProfile` registry. The home opener returns the SAME `PullFeedController`
//! `register_op_feed_defaults` registered under `OP_FEED_SNAPSHOT_KEY`, so a
//! viewport report on the home descriptor drives the existing seq-ordered pull
//! pager — no second controller, no parallel paging path.

use std::sync::Arc;

use nmp_feed::FeedController;

/// The canonical home-feed profile id (ADR-0061). The shell opens
/// `{"profile":"notes","source":{"homeFollowSet":{}},"scope":"activeAccount"}`;
/// it never names the kinds.
pub const HOME_FEED_PROFILE_ID: &str = "notes";

/// Install the `"notes"` [`nmp_feed::FeedProfile`] and the home descriptor
/// opener into `surface`, REUSING the already-registered home `controller`.
///
/// Exposed as a `pub` helper so the surface→pager E2E test can exercise the
/// exact production wiring without standing up a full actor.
pub fn install_home_feed_surface(
    surface: &nmp_feed::FeedSurface,
    controller: Arc<dyn FeedController>,
    event_kinds: &[u32],
) {
    let page_policy = nmp_feed::FeedPagePolicy::default();
    surface.install_profile(nmp_feed::FeedProfile {
        id: nmp_feed::FeedProfileId::from(HOME_FEED_PROFILE_ID),
        event_kinds: event_kinds.iter().copied().collect(),
        renderer: nmp_feed::FeedRenderer::OpRooted,
        page_policy,
    });
    surface.register_opener(Arc::new(HomeFeedOpener {
        controller,
        page_policy,
    }));
}

/// The home descriptor opener: matches the `"notes"` profile with the
/// [`nmp_feed::FeedSource::HomeFollowSet`] source and yields the reused home
/// controller.
struct HomeFeedOpener {
    controller: Arc<dyn FeedController>,
    page_policy: nmp_feed::FeedPagePolicy,
}

impl nmp_feed::FeedOpener for HomeFeedOpener {
    fn open(&self, descriptor: &nmp_feed::FeedDescriptor) -> Option<nmp_feed::OpenedFeed> {
        if descriptor.profile.as_str() == HOME_FEED_PROFILE_ID
            && matches!(descriptor.source, nmp_feed::FeedSource::HomeFollowSet { .. })
        {
            Some(nmp_feed::OpenedFeed {
                controller: Arc::clone(&self.controller),
                page_policy: self.page_policy,
            })
        } else {
            None
        }
    }
}
