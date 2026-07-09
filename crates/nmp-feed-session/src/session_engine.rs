//! The generalized session-engine builder for feed scopes (#1740 step 3).
//!
//! Post-demolition every [`nmp_feed::FeedScope`] compiles to a SINGLE shape:
//! `FeedShape::Flat` over the generic `nmp_feed::FlatFeed<nmp_feed::FeedRow>`.
//! The former `RootIndexed` reply-rollup engine, its `OpFeedEngine` instance,
//! and the op-scope "artifacts" (engine/controller/follow_set diagnostic
//! handles) are DELETED (#3082). There is no second feed engine.
//!
//! # #3092 — one row-building/merge path
//!
//! The `Flat` branch used to build rows via the bespoke
//! `nmp_note_feed::feed_row_builder`/`timeline_merge` pair — a SECOND
//! row-building/merge implementation over the same `FeedRow`, parallel to the
//! composite lane-mapping compiler (`composite_compiler.rs`). It now compiles
//! to the SAME composite mechanism (`composite_compiler::build_composite_rows`/
//! `composite_merge`) via a fixed two-lane default set: `feed.authored` over
//! the app's declared primary kinds, plus a `nip18.target`-style RenderOnly
//! lane over whatever repost-wrapper kind(s) the primary-kind validator
//! derived into the acquisition set (see `flat_lane_set`). `nmp-note-feed` is
//! DELETED; there is no second lane-mapping/merge implementation.
//!
//! # #3117 — suppression re-driven from delivered events
//!
//! Mute/delete SUPPRESSION is no longer applied via a synchronous store peek
//! (the old `OpFeedObserver` cache-luck bug, #3083). `suppression_ingest`
//! re-drives it purely from events the engine has ALREADY been delivered, at
//! both ingest choke points `flat_session` owns. The single-lane `FeedParams`
//! path (this module's `Flat` arm) threads the caller's real
//! `Arc<dyn SuppressionLookup>` through. The composite-lane path
//! (`composite_compiler::open_composite_feed`) does not yet have a caller
//! that supplies one and still passes `empty_suppression_lookup()` — tracked
//! as a #3117 follow-up, not a regression (it matches this path's prior,
//! always-unsuppressed behaviour).
//!
//! Doctrine map:
//! - D0: this names no app product — it consumes a compiled admission predicate
//!   + typed acquisition shapes. Scope→predicate semantics live in `resolve.rs`.
//! - D8: each session's interests are withdrawn on close (symmetric teardown,
//!   in `flat_session`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::SuppressionLookup;
use nmp_feed::{FeedSessionBuild, FeedShape, FeedWindowPolicy};
use nmp_planner::InterestScope;

use super::source::ReducedSource;

mod flat_lane_set;
mod flat_session;
mod suppression_ingest;

pub use flat_lane_set::compile_default_lanes;

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
    primary_kinds: &BTreeSet<u32>,
    acquisition_kinds: &BTreeSet<u32>,
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<ScopeSessionBuild, FeedOpenError> {
    match shape {
        FeedShape::Flat => {
            // The single-scope `FeedParams` path's identity/merge knobs (#3092):
            // compiled onto the SAME composite lane-mapping engine
            // (`crate::composite_compiler`) the multi-lane `open_composite_feed`
            // path uses, via a fixed default two-lane set — see
            // `flat_lane_set::compile_default_lanes`. There is no second
            // row-building/merge implementation.
            let (item_builder, merge) = compile_default_lanes(
                resolved.admission.clone(),
                resolved.row_context.clone(),
                primary_kinds,
                acquisition_kinds,
            );
            // No `Delivered`-ref demand source on the single-lane path — it
            // has no `DeliveredRefDemand` to retract (#3087 is a composite-lane
            // concern only).
            flat_session::build_flat_scope_session(
                app,
                key,
                window,
                resolved,
                item_builder,
                merge,
                None,
                suppression,
            )
            .map(|build| ScopeSessionBuild { build })
        }
    }
}

/// Build a flat-shape feed session over a caller-supplied identity/merge knob
/// pair (#3082 composite-lane compiler entry point). `flat_session` is a
/// private submodule of `session_engine`; this re-export is the ONE crate-visible
/// door into it, so `composite_compiler` shares the exact same session-build
/// mechanics the single-lane `FeedParams` path uses (no second engine wiring).
pub(crate) fn build_flat_scope_session(
    app: &impl FeedSessionHost,
    key: &str,
    window: FeedWindowPolicy,
    resolved: ReducedSource,
    item_builder: nmp_feed::FlatFeedItemBuilder<nmp_feed::FeedRow>,
    merge: nmp_feed::FlatFeedMerge<nmp_feed::FeedRow>,
    // Fired when the engine drops a source contribution (#3087). `None` for
    // the single-lane `FeedParams` path (no demand source to retract); the
    // composite-lane compiler passes its `DeliveredRefDemand::retract_source`
    // closure.
    source_removed: Option<nmp_feed::SourceRemovedHook>,
    // #3117: re-drives mute/delete suppression from delivered events at both
    // ingest choke points `flat_session` owns. See this module's doc comment
    // for why the composite-lane caller still passes an empty lookup.
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    flat_session::build_flat_scope_session(
        app,
        key,
        window,
        resolved,
        item_builder,
        merge,
        source_removed,
        suppression,
    )
}

pub(super) fn visible_flat_payload(feed: &nmp_feed::FlatFeed<nmp_feed::FeedRow>) -> Vec<u8> {
    let snapshot = feed.snapshot_current_window();
    nmp_feed::typed_wire::encode_feed_row_snapshot(&snapshot)
}

pub(crate) fn interest_scope_code(scope: InterestScope) -> u32 {
    match scope {
        InterestScope::ActiveAccount | InterestScope::Account(_) => 0,
        InterestScope::Global => 1,
    }
}
