//! Composite multi-lane feed open mechanics for UniFFI facades (#3086).
//!
//! `NmpApp::open_composite_feed` (`crates/nmp-native-runtime/src/composite_feed.rs`)
//! is the composition-root entry point for #3082's composite multi-lane feed
//! engine, but before this module it had no binding-surface exposure — a
//! Swift/Kotlin/TS shell could not open one. This mirrors `sessions.rs`'s
//! `open_feed` exactly: decode + open from JSON, returning the SAME
//! [`super::sessions::OpenedFeed`] handle shape `close_feed`/`load_older_feed`/
//! `reopen_feed` already accept, because a composite feed session is recorded
//! in the identical engine-agnostic session registry `open_feed` uses
//! (`NmpApp::feed_sessions`) — there is no separate composite close/page path
//! to add.
//!
//! Feature-gated behind `composite-feed` (mirrors how `nmp-native-runtime`
//! itself gates the composition root behind the same feature name, #2797):
//! an app that never declares a composite feed does not pull nmp-nip18/
//! nmp-nip22 into its binary through this crate either.
//!
//! Live end-to-end hydration of a feed opened through this surface (a real
//! relay delivering events into a freshly-opened composite session) is gated
//! on the separate nmp-core observed-projection dispatch gap tracked as #3088
//! — out of scope here; this module only proves the JSON round-trip and the
//! open/register/teardown lifecycle, the same scope `sessions.rs`'s own
//! `open_feed` tests cover for the plain feed path.

use nmp_native_runtime::{CompositeFeedParams, NmpApp};

use crate::sessions::{FeedError, OpenedFeed};

/// Decode + validate + open a composite multi-lane feed from JSON params.
///
/// Uses `NmpApp::open_composite_feed`, so the canonical composite compiler
/// (and its lane-mapping registry) stays below the app facade boundary,
/// exactly like [`super::sessions::open_feed`] keeps the plain-feed compiler
/// below the boundary.
///
/// # Errors
///
/// * [`FeedError::InvalidParams`] — `params_json` is not valid JSON for
///   [`CompositeFeedParams`].
/// * [`FeedError::OpenFailed`] — the runtime could not compile/register the
///   composite feed (an unregistered lane mapping, an unsupported lane scope,
///   or a poisoned session registry).
pub fn open_composite_feed(app: &NmpApp, params_json: &str) -> Result<OpenedFeed, FeedError> {
    let params: CompositeFeedParams =
        serde_json::from_str(params_json).map_err(|_| FeedError::InvalidParams)?;

    app.open_composite_feed(&params)
        .map(|handle| OpenedFeed {
            projection_key: handle.projection_key.into_string(),
            handle_id: handle.session_id.0,
        })
        .map_err(|_| FeedError::OpenFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::close_feed;

    const SINGLE_LANE_COMPOSITE: &str = r#"{
        "key": "app.composite.support.test",
        "lanes": [{
            "source": {"Authors": {"authors": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]}},
            "match_kinds": [1],
            "match_tags": {},
            "mapping": "feed.authored"
        }],
        "render_target_kinds": [],
        "sort": "ByInteractionTime",
        "window": {"initial_limit": 80},
        "item_projection": "FeedRows"
    }"#;

    #[test]
    fn open_rejects_malformed_json() {
        let app = nmp_native_runtime::new_app();
        assert_eq!(
            open_composite_feed(&app, "{not json}"),
            Err(FeedError::InvalidParams)
        );
    }

    /// The load-bearing proof: `params_json` round-trips through
    /// `CompositeFeedParams`'s real `Deserialize` impl and actually opens a
    /// live composite session through the real composition root — the same
    /// registry `open_feed` records into, so `close_feed` (unchanged, shared
    /// with the plain-feed path) tears it down.
    #[test]
    fn open_then_close_round_trips_through_the_real_composition_root() {
        let app = nmp_native_runtime::new_app();
        let Ok(opened) = open_composite_feed(&app, SINGLE_LANE_COMPOSITE) else {
            assert!(
                false,
                "a single-lane composite feed must open through the real composition root"
            );
            return;
        };
        assert!(!opened.projection_key.is_empty());
        assert_ne!(opened.handle_id, 0);
        assert_eq!(app.live_feed_session_count(), 1);

        assert!(
            close_feed(&app, &opened),
            "the shared close_feed mechanic tears down a composite session too"
        );
        assert_eq!(app.live_feed_session_count(), 0);
        assert!(
            !close_feed(&app, &opened),
            "second close is a no-op (D6), same contract as the plain-feed path"
        );
    }

    #[test]
    fn open_fails_closed_on_unregistered_mapping() {
        let app = nmp_native_runtime::new_app();
        let json = SINGLE_LANE_COMPOSITE.replace("feed.authored", "nothing.registered");
        assert_eq!(open_composite_feed(&app, &json), Err(FeedError::OpenFailed));
        assert_eq!(app.live_feed_session_count(), 0);
    }
}
