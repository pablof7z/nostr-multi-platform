//! `open_active_follows_op_feed` — composition helper that opens the active
//! account's follows timeline.
//!
//! # Post-demolition (#3082)
//!
//! This USED to wire a bespoke `OpFeedEngine` (the RootIndexed reply-rollup
//! engine) plus a diagnostic `ActiveFollowSet` handle. That engine is DELETED.
//! The follows timeline is now an ORDINARY flat feed opened through the same
//! `open_feed(FeedScope::ActiveUserFollows)` path as every other scope, with the
//! generic `nmp_feed::FlatFeed<FeedRow>` engine. Acquisition, follow-set
//! reactivity, and window reset are owned by the reduced-source resolver — this
//! helper only builds the active-follows [`FeedParams`] and opens them.
//!
//! The returned session therefore exposes ONLY the feed handle; the former
//! `engine` / `controller` / `follow_set` diagnostic handles are gone (they were
//! RootIndexed-engine internals).

use crate::{FeedOpenError, NmpApp};
use nmp_core::substrate::SuppressionLookup;
use nmp_feed::{
    FeedAdmission, FeedHandle, FeedOrder, FeedParams, FeedScope, FeedShape, FeedWindowPolicy,
    ProjectionKey,
};
use nmp_nip51::MuteListProjection;
use std::collections::BTreeSet;
use std::sync::Arc;

#[cfg(test)]
mod active_shape;
#[cfg(test)]
use active_shape::live_active_follows_shape;

#[cfg(test)]
use nmp_core::slots::ActiveAccountSlot;

type Pubkey = String;

/// What [`open_active_follows_op_feed`] hands back to the composition caller.
pub struct ActiveFollowsOpFeedSession {
    /// The ordinary feed-session handle for the caller-owned projection.
    ///
    /// `None` means the typed declaration failed closed before registration.
    pub handle: Option<FeedHandle>,
}

/// Wire an active-follows flat feed session into `app`.
///
/// # Ordering
///
/// Call before `nmp_app_start`: the session's observers must be visible to the
/// kernel when the first event arrives.
pub fn open_active_follows_op_feed(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
) -> ActiveFollowsOpFeedSession {
    let _ = viewer; // carried for API parity; the flat session is viewer-agnostic
    let params = active_follows_op_feed_params(primary_feed_kinds, projection);
    open_flat(app, &params)
}

/// Wire an active-follows flat feed with the NIP-51 mute read model.
///
/// Resets the current feed window whenever the active account's mute list
/// replacement changes, AND (#3117) threads `mute` through as the session's
/// real `SuppressionLookup` — `MuteListProjection` already IS one, so the
/// reset stops being cosmetic: a refill after the window reset now actually
/// re-applies the current mute state, on top of the delivery-time suppression
/// pass every live/backfill event goes through (`suppression_ingest`).
pub fn open_active_follows_op_feed_with_mute(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    projection: ProjectionKey,
    mute: Arc<MuteListProjection>,
) -> ActiveFollowsOpFeedSession {
    let _ = viewer;
    let params = active_follows_op_feed_params(primary_feed_kinds, projection.clone());
    let suppression: Arc<dyn SuppressionLookup> = mute.clone();
    let session = open_flat_with_suppression(app, &params, suppression);
    if session.handle.is_some() {
        let registry = app.feed_registry_handle();
        let sender = app.command_sender();
        let projection_key = projection.as_str().to_string();
        mute.on_change(Box::new(move || {
            if registry.reset(&projection_key) {
                sender.mark_changed_since_emit();
            }
        }));
    }
    session
}

fn open_flat(app: &NmpApp, params: &FeedParams) -> ActiveFollowsOpFeedSession {
    ActiveFollowsOpFeedSession {
        handle: app.open_feed(params).ok(),
    }
}

/// Same as [`open_flat`], but compiles through `compile_feed_params_with_suppression`
/// with a real `Arc<dyn SuppressionLookup>` instead of `NmpApp::open_feed`'s
/// default (empty) compiler — the seam #3117 needed to actually reach a
/// production caller.
fn open_flat_with_suppression(
    app: &NmpApp,
    params: &FeedParams,
    suppression: Arc<dyn SuppressionLookup>,
) -> ActiveFollowsOpFeedSession {
    let compiler = move |app: &NmpApp, params: &FeedParams, acquisition_kinds: &BTreeSet<u32>| {
        nmp_feed_session::compile_feed_params_with_suppression(
            app,
            params,
            acquisition_kinds,
            Arc::clone(&suppression),
        )
    };
    ActiveFollowsOpFeedSession {
        handle: app.open_feed_with_compiler(params, &compiler).ok(),
    }
}

#[must_use]
pub fn active_follows_op_feed_params(
    primary_feed_kinds: Vec<u32>,
    key: ProjectionKey,
) -> FeedParams {
    FeedParams {
        primary_kinds: primary_feed_kinds,
        shape: FeedShape::Flat,
        source: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        order: FeedOrder::NewestByFeedPosition,
        window: FeedWindowPolicy::bounded(nmp_feed::DEFAULT_FEED_WINDOW_LIMIT),
        key,
        item_projection: nmp_feed::FeedItemProjection::FeedRows,
    }
}

#[cfg(test)]
#[path = "op_feed_session/tests.rs"]
mod tests;
