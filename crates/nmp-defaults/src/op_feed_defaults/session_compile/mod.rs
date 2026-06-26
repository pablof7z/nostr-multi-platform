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

use nmp_feed::{FeedAdmission, FeedParams, FeedSessionBuild};
use nmp_ffi::{FeedOpenError, NmpApp};

mod custom;
mod flat_replay;
mod nip51_sources;
mod resolve;
mod resolve_static;
mod session_engine;
mod set_algebra;
mod source;
mod source_replay;
mod wot_graph;

#[cfg(test)]
mod source_tests;
#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;

/// Compile a [`FeedParams`] into a registered feed session over the EXISTING
/// op-feed mechanics, returning the teardown recipe `open_feed` records.
///
/// # Wired vs deferred scopes (step 2)
///
/// Every [`nmp_feed::FeedScope`] variant routes through the reduced-source
/// compiler. `ActiveUserFollows` is not a special door: it resolves to the same
/// session-owned dependent-interest shape as `ContactList`, `ListMembers`, and
/// set algebra, with the active-account source re-resolved by the identity
/// observer.
///
/// # Step 3 — every scope goes through ONE compiler
///
/// * `ActiveUserFollows` → live [`nmp_nip02::ActiveFollowSet`] predicate and
///   session-owned dependent acquisition; opens before sign-in and recompiles
///   once the active-account slot is populated.
/// * `ContactList` (active owner) → same live follow-set source with a concrete
///   active-owner check (foreign owner fails closed — no single-source resolver
///   yet).
/// * `ListMembers` → live NIP-51 pubkey reducers:
///   [`nmp_nip51::PeopleListProjection`] for kind:30000 list ids, and
///   [`nmp_nip51::MuteListProjection`] when the list id is
///   [`nmp_nip51::ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID`].
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
