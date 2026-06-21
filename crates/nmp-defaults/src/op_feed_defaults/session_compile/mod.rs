//! The `FeedParams` → registered-session compiler (#1740 steps 2 + 3).
//!
//! THE composition-layer compiler [`nmp_ffi::NmpApp::open_feed`] drives. It
//! names both `NmpApp` and the op-feed instance in one breath (the same edge
//! [`super::register_op_feed_defaults`] owns) — exactly why it lives here and not
//! in `nmp-ffi` (D0: `nmp-ffi` matches on no `FeedScope`). It is a SESSION
//! WRAPPER over the existing home-feed mechanics, not a second feed engine (D4).
//!
//! Step 3 added the CLOSED perspective compiler: every non-default scope routes
//! through ONE path — `resolve::resolve_scope` compiles the typed scope into a
//! COMPILED admission predicate ([`nmp_feed::AdmitExpr`] / a live framework
//! projection — never an app closure) plus internal acquisition interests, and
//! `session_engine::build_scope_session` registers a session engine under the
//! caller's unique projection key. Set algebra (`Union`/`Intersection`/
//! `Difference`) composes child compilations in `set_algebra`.

use std::collections::BTreeSet;

use nmp_feed::{FeedAdmission, FeedParams, FeedRanking, FeedScope, FeedSessionBuild};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::{read_active, register_op_feed_defaults};

mod resolve;
mod session_engine;
mod set_algebra;

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;

/// Compile a [`FeedParams`] into a registered feed session over the EXISTING
/// op-feed mechanics, returning the teardown recipe `open_feed` records.
///
/// # Wired vs deferred scopes (step 2)
///
/// * [`FeedScope::ActiveUserFollows`] — WIRED. Reuses
///   [`super::register_op_feed_defaults`] verbatim (engine + pull controller +
///   typed NOFS sidecar + follow-set/engine observers + the actor-owned
///   active-follows acquisition declaration), then builds a handle-based
///   teardown that unregisters the `nmp.feed.home` controller + projection,
///   revokes both captured observer ids, AND clears the active-follows feed
///   interests (the close-side of the open path's `declare_active_follows_feed`)
///   before a final change-notification. No new feed engine.
/// * Every other [`FeedScope`] variant (`ContactList`, `ListMembers`, `Wot`,
///   `RelaySet`, `Tag`, set algebra, `CustomPerspectiveId`) — DEFERRED to step 3
///   (the full perspective compiler). They fail closed here with
///   [`FeedOpenError::ScopeNotSupportedYet`] WITHOUT registering anything, so
///   there is nothing to leak.
///
/// # Step 3 — every non-default scope goes through ONE compiler
///
/// * `ContactList` (active owner) → live [`nmp_nip02::ActiveFollowSet`] predicate
///   (foreign owner fails closed — no single-source resolver yet).
/// * `ListMembers` → live [`nmp_nip51::PeopleListProjection`] (kind:30000)
///   member predicate.
/// * `Wot` → the #1698 [`nmp_wot::score::WotGraph`] ranked second-degree query.
/// * `Tag` → `#t` acquisition with `Any` admission (the filter gates).
/// * `Union`/`Intersection`/`Difference` → set algebra over the compiled
///   children.
/// * `RelaySet` and `CustomPerspectiveId` stay fail-closed (no resolver / step
///   4 respectively).
pub fn compile_feed_params(
    app: &NmpApp,
    params: &FeedParams,
    acquisition_kinds: &BTreeSet<u32>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    // FAIL CLOSED on app-defined admission / ranking (#1740 step 3). The
    // compiler today wires only the ACQUISITION scope's compiled perspective +
    // the built-in chronological ranking. A `FeedAdmission::Custom` /
    // `FeedRanking::Custom` names an app-registered perspective whose
    // registration mechanism lands in step 4 — until then there is no compiled
    // predicate for it, so opening with default behavior would SILENTLY open the
    // feed wider than the app declared. Reject before registering anything (D6).
    if !matches!(params.admission, FeedAdmission::All) {
        return not_supported_yet("custom-admission");
    }
    // The session engine sorts roots newest-first (`ChronologicalDesc`) only;
    // `ChronologicalAsc` and `Custom` are not wired, so anything but the default
    // descending order would silently mis-order — fail closed.
    if !matches!(params.ranking, FeedRanking::ChronologicalDesc) {
        return not_supported_yet("custom-ranking");
    }

    match &params.acquisition {
        // The framework-default home perspective keeps its dedicated wiring.
        FeedScope::ActiveUserFollows => compile_active_user_follows(app, params),
        // Step 4 lands the app-defined-perspective registration mechanism.
        FeedScope::CustomPerspectiveId(_) => not_supported_yet("CustomPerspectiveId"),
        // Every other scope: resolve the typed scope into a compiled admission
        // predicate + internal interests, then register a session engine under
        // the unique projection key.
        scope => {
            let resolved = resolve::resolve_scope(app, scope, acquisition_kinds)?;
            session_engine::build_scope_session(app, &params.projection.0, resolved)
        }
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
    //
    // EXECUTION ORDER (the contract this recipe encodes):
    //   1. unregister the feed controller          ── registry removals
    //   2. revoke the engine ingest observer        ──        ↓
    //   3. revoke the follow-set observer           ──        ↓
    //   4. remove the typed sidecar projection      ── registry removals end
    //   5. clear the active-follows interests       ── WITHDRAW actor-owned state
    //   6. mark-changed (the change notification)   ── RUN LAST, after all the above
    //
    // The `FeedSessionRegistry` runs the teardown Vec in REVERSE registration
    // order (`session.rs::run_teardown`). So to make the change-notification run
    // LAST in execution order it is placed FIRST in the Vec below, and the
    // controller-unregister (which must run FIRST in execution order) is placed
    // LAST. `clear_active_follows` (fix #1740 — the close-side of the open path's
    // `declare_active_follows_feed`) sits next to mark-changed: after the
    // registry removals, before the notify, so the interest withdrawal + state
    // clear are in flight before the snapshot tick that mark-changed forces.
    // Symmetric with the open path: `register_op_feed_defaults` issued
    // `DeclareActiveFollowsFeed`; this issues `ClearActiveFollowsFeed`.
    let key = nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY;
    let teardown = app.feed_teardown();
    let [follow_set_observer_id, engine_observer_id] = defaults.observer_ids;
    Ok(FeedSessionBuild {
        projection_key: params.projection.clone(),
        // Registration order = REVERSE of the execution order documented above
        // (the registry reverses the Vec on close).
        teardown: vec![
            teardown.mark_changed(),                       // exec #6 (runs last)
            teardown.clear_active_follows(),               // exec #5
            teardown.remove_projection(key),               // exec #4
            teardown.revoke_observer(follow_set_observer_id), // exec #3
            teardown.revoke_observer(engine_observer_id),  // exec #2
            teardown.unregister_feed(key),                 // exec #1 (runs first)
        ],
    })
}
