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
//!
//! Step 4 adds `CustomPerspectiveId` RESOLUTION over the same compiler: an app
//! registers a CLOSED [`nmp_feed::CustomPerspectiveDef`] (a `FeedScope` +
//! ranking) under an id (`NmpApp::register_custom_perspective`); a `Custom`
//! reference in [`FeedParams`] looks the id up and compiles the registered scope
//! through `resolve_scope`/`build_scope_session` — NO second resolver. An
//! UNREGISTERED id still fails CLOSED (no leak). See `custom.rs`.

use std::collections::BTreeSet;

use nmp_feed::{FeedAdmission, FeedParams, FeedScope, FeedSessionBuild};
use nmp_ffi::{FeedOpenError, NmpApp};

use super::{read_active, register_op_feed_defaults};

mod custom;
mod resolve;
mod resolve_static;
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
/// * `Tag` → `#t` acquisition with EVENT-AWARE `AdmitExpr::Tag` admission (the
///   filter gates at acquisition, but admission re-checks the tag so the scope
///   composes faithfully inside set algebra — see `resolve::resolve_tag`).
/// * `Union`/`Intersection`/`Difference` → set algebra over the compiled
///   children.
/// * `RelaySet` and `CustomPerspectiveId` stay fail-closed (no resolver / step
///   4 respectively).
pub fn compile_feed_params(
    app: &NmpApp,
    params: &FeedParams,
    acquisition_kinds: &BTreeSet<u32>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    // RANKING (#1740 step 4). The session engine sorts roots newest-first
    // (`ChronologicalDesc`) only. `ChronologicalAsc` is not wired. A
    // `FeedRanking::Custom(id)` resolves to a REGISTERED perspective's ranking —
    // which must itself be engine-honorable (`ChronologicalDesc`) or the open
    // fails closed. Anything the engine cannot honor would silently mis-order, so
    // reject before registering anything (D6). An UNREGISTERED id also fails
    // closed (no leak). `custom::resolve_ranking` returns the engine-honored
    // order or a typed error.
    custom::resolve_ranking(app, &params.ranking)?;

    // The framework-default home perspective keeps its dedicated wiring. It does
    // not support a custom admission gate (the home path is its own engine), so a
    // custom admission over `ActiveUserFollows` fails closed.
    if matches!(params.acquisition, FeedScope::ActiveUserFollows) {
        if !matches!(params.admission, FeedAdmission::All) {
            return not_supported_yet("custom-admission");
        }
        return compile_active_user_follows(app, params);
    }

    // ── Resolve the ACQUISITION scope (step 3 compiler; custom id → registered
    //    definition's scope). An unregistered `CustomPerspectiveId` fails closed.
    let mut resolved = custom::resolve_acquisition(app, &params.acquisition, acquisition_kinds)?;

    // ── ADMISSION. `All` keeps the acquisition's own admission gate; `Custom(id)`
    //    intersects the registered perspective's compiled admission ON TOP (a
    //    pure filter — it adds no row source, like `Difference`'s right side). An
    //    unregistered id fails closed; the acquisition's already-registered
    //    resolver observers are revoked so nothing leaks.
    if let FeedAdmission::Custom(id) = &params.admission {
        resolved = custom::apply_custom_admission(app, resolved, id, acquisition_kinds)?;
    }

    session_engine::build_scope_session(app, &params.projection.0, &params.render, resolved)
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
    // The viewer is the active account when one exists. A view-driven shell may
    // open the home feed before sign-in; `register_op_feed_defaults` already
    // reads the live active-account slot for acquisition/pull and self-seeds
    // once identity arrives, so use an empty bootstrap viewer in that window
    // instead of dropping the declaration.
    let viewer = read_active(&app.active_account_handle()).unwrap_or_default();

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
            teardown.mark_changed(),                          // exec #6 (runs last)
            teardown.clear_active_follows(),                  // exec #5
            teardown.remove_projection(key),                  // exec #4
            teardown.revoke_observer(follow_set_observer_id), // exec #3
            teardown.revoke_observer(engine_observer_id),     // exec #2
            teardown.unregister_feed(key),                    // exec #1 (runs first)
        ],
    })
}
