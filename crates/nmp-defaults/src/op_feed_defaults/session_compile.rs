//! #1740 step 2 — the `FeedParams` → existing-registration compiler.
//!
//! THE composition-layer compiler [`nmp_ffi::NmpApp::open_feed`] drives. It
//! names both `NmpApp` and the op-feed instance in one breath (the same edge
//! [`super::register_op_feed_defaults`] owns) — exactly why it lives here and not
//! in `nmp-ffi` (D0: `nmp-ffi` matches on no `FeedScope`). It is a SESSION
//! WRAPPER over the existing home-feed mechanics, not a second feed engine (D4).

use std::collections::BTreeSet;

use nmp_feed::{FeedParams, FeedScope, FeedSessionBuild};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::{read_active, register_op_feed_defaults};

/// Compile a [`FeedParams`] into a registered feed session over the EXISTING
/// op-feed mechanics, returning the teardown recipe `open_feed` records.
///
/// # Wired vs deferred scopes (step 2)
///
/// * [`FeedScope::ActiveUserFollows`] — WIRED. Reuses
///   [`super::register_op_feed_defaults`] verbatim (engine + pull controller +
///   typed NOFS sidecar + follow-set/engine observers), then builds a
///   handle-based teardown that revokes both captured observer ids and
///   unregisters the `nmp.feed.home` controller + projection. No new feed engine.
/// * Every other [`FeedScope`] variant (`ContactList`, `ListMembers`, `Wot`,
///   `RelaySet`, `Tag`, set algebra, `CustomPerspectiveId`) — DEFERRED to step 3
///   (the full perspective compiler). They fail closed here with
///   [`FeedOpenError::ScopeNotSupportedYet`] WITHOUT registering anything, so
///   there is nothing to leak.
///
/// `ContactList`/`author` and `thread` are listed in the issue as
/// existing-mechanic scopes, but their concrete registration is the per-app
/// author/thread `FlatFeed` seam (`nmp_app_chirp_open_author_feed`), which is
/// not part of the framework-default composition this crate owns. They are
/// therefore wired by the full perspective compiler in step 3 alongside the
/// other variants; step 2 ships the framework-default `ActiveUserFollows` home
/// session, fail-closed for the rest. (Fail closed, documented — per the issue.)
pub fn compile_feed_params(
    app: &NmpApp,
    params: &FeedParams,
    _acquisition_kinds: &BTreeSet<u32>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    match &params.acquisition {
        FeedScope::ActiveUserFollows => compile_active_user_follows(app, params),
        FeedScope::ContactList { .. } => not_supported_yet("ContactList"),
        FeedScope::ListMembers { .. } => not_supported_yet("ListMembers"),
        FeedScope::Wot { .. } => not_supported_yet("Wot"),
        FeedScope::RelaySet { .. } => not_supported_yet("RelaySet"),
        FeedScope::Tag { .. } => not_supported_yet("Tag"),
        FeedScope::Union(..) => not_supported_yet("Union"),
        FeedScope::Intersection(..) => not_supported_yet("Intersection"),
        FeedScope::Difference(..) => not_supported_yet("Difference"),
        FeedScope::CustomPerspectiveId(_) => not_supported_yet("CustomPerspectiveId"),
    }
}

fn not_supported_yet(scope: &'static str) -> Result<FeedSessionBuild, FeedOpenError> {
    Err(FeedOpenError::ScopeNotSupportedYet { scope })
}

/// Wire the framework-default active-follows home feed over the existing
/// mechanics and return its handle-based teardown recipe.
fn compile_active_user_follows(
    app: &NmpApp,
    params: &FeedParams,
) -> Result<FeedSessionBuild, FeedOpenError> {
    // The viewer is the active account (the reactive perspective owner). No live
    // account ⇒ fail closed: an active-follows feed has no viewer to anchor.
    let viewer = read_active(&app.active_account_handle()).ok_or(
        FeedOpenError::ScopeNotSupportedYet {
            scope: "ActiveUserFollows-no-active-account",
        },
    )?;

    // Reuse the EXISTING composition verbatim — engine + pull controller + typed
    // NOFS sidecar + observers. `register_op_feed_defaults` derives wrapper
    // acquisition below the app boundary; we hand it the validated primary kinds.
    let defaults = register_op_feed_defaults(app, viewer, params.primary_kinds.clone());

    // Handle-based teardown over the SAME registry primitives the wiring used.
    // Reverse-run on close: projection, observers, controller, then mark-changed.
    let key = nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY;
    let teardown = app.feed_teardown();
    let [follow_set_observer_id, engine_observer_id] = defaults.observer_ids;
    Ok(FeedSessionBuild {
        projection_key: params.projection.clone(),
        teardown: vec![
            teardown.unregister_feed(key),
            teardown.revoke_observer(follow_set_observer_id),
            teardown.revoke_observer(engine_observer_id),
            teardown.remove_projection(key),
            teardown.mark_changed(),
        ],
    })
}
