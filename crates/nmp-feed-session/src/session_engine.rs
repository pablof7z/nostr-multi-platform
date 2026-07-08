//! The generalized session-engine builder for feed scopes (#1740 step 3).
//!
//! Post-demolition every [`nmp_feed::FeedScope`] compiles to a SINGLE shape:
//! `FeedShape::Flat` over the generic `nmp_feed::FlatFeed<nmp_feed::FeedRow>`.
//! The former `RootIndexed` reply-rollup engine, its `OpFeedEngine` instance,
//! and the op-scope "artifacts" (engine/controller/follow_set diagnostic
//! handles) are DELETED (#3082). There is no second feed engine.
//!
//! TODO(#3082): mute/delete SUPPRESSION is no longer applied inside the feed
//! (the old `OpFeedObserver` did a synchronous store peek — the cache-luck bug
//! #3083). Suppression must be re-driven by DELIVERED mute/delete events; that
//! wiring is not in this PR. The `suppression` argument is currently unused.
//!
//! Doctrine map:
//! - D0: this names no app product — it consumes a compiled admission predicate
//!   + typed acquisition shapes. Scope→predicate semantics live in `resolve.rs`.
//! - D8: each session's interests are withdrawn on close (symmetric teardown,
//!   in `flat_session`).

use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::SuppressionLookup;
use nmp_feed::{FeedSessionBuild, FeedShape, FeedWindowPolicy};
use nmp_planner::InterestScope;

use super::source::ReducedSource;

mod flat_session;

/// The compiled session build. (The former `artifacts` field carried the deleted
/// `OpFeedEngine`; there are no engine artifacts to hand back anymore.)
pub struct ScopeSessionBuild {
    pub build: FeedSessionBuild,
}

/// Build a registered feed session for a reduced source and return its teardown
/// recipe. Every scope compiles to the single flat shape.
pub(super) fn build_scope_session_with_artifacts(
    app: &impl FeedSessionHost,
    key: &str,
    shape: &FeedShape,
    window: FeedWindowPolicy,
    resolved: ReducedSource,
    // TODO(#3082): suppression must be driven by delivered mute/delete events,
    // not a synchronous store lookup. Unused until that lane is rewired.
    _suppression: Arc<dyn SuppressionLookup>,
) -> Result<ScopeSessionBuild, FeedOpenError> {
    match shape {
        FeedShape::Flat => flat_session::build_flat_scope_session(app, key, window, resolved)
            .map(|build| ScopeSessionBuild { build }),
    }
}

pub(super) fn visible_flat_payload(feed: &nmp_feed::FlatFeed<nmp_feed::FeedRow>) -> Vec<u8> {
    let snapshot = feed.snapshot_current_window();
    nmp_note_feed::encode_feed_row_snapshot(&snapshot)
}

pub(crate) fn interest_scope_code(scope: InterestScope) -> u32 {
    match scope {
        InterestScope::ActiveAccount | InterestScope::Account(_) => 0,
        InterestScope::Global => 1,
    }
}
