//! Shared `FeedParams` → registered-session compiler.
//!
//! Runtime composition roots drive this compiler through [`FeedSessionHost`].
//! The compiler owns feed-scope semantics, OP/flat session wiring, source
//! effects, dependent acquisition replacement, and typed sidecar registration.
//! Native and browser runtimes adapt their slots/registries into the host trait
//! instead of carrying separate feed-source policy.
//!
//! Step 3 added the CLOSED perspective compiler: every feed scope routes
//! through ONE path — `resolve::resolve_scope` compiles the typed scope into a
//! COMPILED admission predicate ([`nmp_feed::AdmitExpr`] / a live framework
//! projection — never an app closure) plus internal acquisition interests, and
//! `session_engine::build_scope_session` registers a session engine under the
//! caller's unique projection key. Set algebra (`Union`/`Intersection`/
//! `Difference`) composes child compilations in `set_algebra`.
//!
//! Step 4 adds `CustomPerspectiveId` RESOLUTION over the same compiler: an app
//! registers a CLOSED [`nmp_feed::CustomPerspectiveDef`] (a `FeedScope` +
//! ranking) under an id; a `Custom`
//! reference in [`FeedParams`] looks the id up and compiles the registered scope
//! through `resolve_scope`/`build_scope_session` — NO second resolver. An
//! UNREGISTERED id still fails CLOSED (no leak). See `custom.rs`.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{
    empty_suppression_lookup, ObservedProjectionCommandHandle, ObservedProjectionRegistrar,
    SuppressionLookup,
};
use nmp_core::{CommandSender, TypedProjectionData};
use nmp_feed::{
    CustomPerspectiveDef, CustomPerspectiveId, FeedAdmission, FeedAuthorRefs, FeedController,
    FeedParams, FeedSessionBuild, FeedWindowSource, PullFn, TeardownAction,
};

mod active_shape;
mod custom;
mod dynamic_observer;
mod flat_replay;
mod nip29_group_sources;
mod nip51_sources;
mod observed_source;
mod pointer_targets;
mod resolve;
mod resolve_static;
#[cfg(test)]
mod resolve_tests;
mod session_engine;
mod set_algebra;
mod source;
mod source_replay;
mod trellis_adapter;
#[cfg(test)]
mod trellis_adapter_equivalence_support;
#[cfg(test)]
mod trellis_adapter_equivalence_tests;
#[cfg(test)]
mod trellis_adapter_tests;
// #2629 owns the private taxonomy; #2630 is the first production adapter user.
mod trellis_resources;
#[cfg(test)]
mod trellis_resources_tests;
mod wot_graph;
pub(crate) use active_shape::read_active;
pub use observed_source::{compile_observed_feed_source, ObservedFeedSourceOptions};
pub use session_engine::OpScopeSessionArtifacts;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

/// Session-scoped identity observer id used by feed-source resolvers.
pub type IdentityChangeObserverId = u64;

/// Typed failure of a feed-session compile (D6 — no panic across runtime seams).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedOpenError {
    /// The declared [`FeedParams`] failed primary-kind validation.
    InvalidParams(nmp_nip18::PrimaryKindError),
    /// The declared scope is recognized by the model but unsupported by this
    /// runtime/capability set.
    ScopeNotSupportedYet { scope: &'static str },
    /// The feed-session registry could not track the compiled session; the
    /// caller must run the just-produced teardown before returning this error.
    RegistryUnavailable,
}

/// Runtime capabilities required by the shared feed-session compiler.
pub trait FeedSessionHost {
    fn active_account_handle(&self) -> nmp_core::slots::ActiveAccountSlot;
    fn event_store_handle(&self) -> nmp_core::slots::EventStoreSlot;
    fn observed_projection_handle(&self) -> ObservedProjectionCommandHandle;
    fn register_identity_change_observer<F>(&self, callback: F) -> IdentityChangeObserverId
    where
        F: Fn(Option<String>) + Send + Sync + 'static;
    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        self.observed_projection_handle()
            .observed_projection_registrar_handle()
    }
    fn unregister_identity_change_observer_action(
        &self,
        id: IdentityChangeObserverId,
    ) -> TeardownAction;
    fn feed_pull_fn(&self) -> PullFn;
    fn command_sender(&self) -> CommandSender;
    fn register_feed(&self, key: String, controller: Arc<dyn FeedController>);
    fn load_older_feed(&self, key: &str) -> bool;
    fn register_feed_window_source<S, F>(
        &self,
        feed_key: String,
        source: Arc<FeedWindowSource<S>>,
        encode: F,
    ) where
        S: FeedAuthorRefs + Send + Sync + 'static,
        F: Fn(&S) -> Option<TypedProjectionData> + Send + Sync + 'static;
    fn custom_perspective(&self, id: &CustomPerspectiveId) -> Option<CustomPerspectiveDef>;
    fn unregister_feed_action(&self, key: String) -> TeardownAction;
    fn remove_projection_action(&self, key: String) -> TeardownAction;
    fn mark_changed_action(&self) -> TeardownAction;
}

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
/// * `ActiveUserHostedGroups` → the active account's kind:10009 NIP-51 list,
///   reduced into one host-pinned NIP-29 `#h` source per relay.
/// * `RelaySet` and `CustomPerspectiveId` stay fail-closed (no resolver / step
///   4 respectively).
pub fn compile_feed_params<H: FeedSessionHost>(
    app: &H,
    params: &FeedParams,
    acquisition_kinds: &BTreeSet<u32>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    compile_feed_params_with_suppression(app, params, acquisition_kinds, empty_suppression_lookup())
}

pub fn compile_feed_params_with_suppression<H: FeedSessionHost>(
    app: &H,
    params: &FeedParams,
    acquisition_kinds: &BTreeSet<u32>,
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    compile_feed_params_with_suppression_and_artifacts(app, params, acquisition_kinds, suppression)
        .map(|detailed| detailed.build)
}

pub fn compile_feed_params_with_suppression_and_artifacts<H: FeedSessionHost>(
    app: &H,
    params: &FeedParams,
    acquisition_kinds: &BTreeSet<u32>,
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<session_engine::ScopeSessionBuild, FeedOpenError> {
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

    session_engine::build_scope_session_with_artifacts(
        app,
        params.projection.as_str(),
        &params.shape,
        resolved,
        suppression,
    )
}
