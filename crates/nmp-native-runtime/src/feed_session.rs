//! `NmpApp::open_feed` / `close_feed` — the feed-session registry seam (#1740
//! step 2).
//!
//! ONE [`NmpApp::open_feed`] call owns a feed's full lifecycle: it validates the
//! declaration, runs the canonical NMP feed compiler, mints a session id +
//! projection key, records the resulting teardown recipe in the engine-agnostic
//! [`nmp_feed::FeedSessionRegistry`], and returns a [`nmp_feed::FeedHandle`].
//! [`NmpApp::close_feed`] looks the session up by the handle's id and tears it
//! ALL down — observer, projection, pull controller, interests — idempotently,
//! using the HANDLE (never a re-derived filter).
//!
//! Tests and internal composition seams may inject a [`FeedCompiler`] through
//! [`NmpApp::open_feed_with_compiler`]. Normal app/native callers do not choose a
//! compiler; they pass a [`FeedParams`] declaration and the runtime applies the
//! canonical compiler below the app boundary.
//!
//! Doctrine map:
//! - D0: app/native callers match on no `FeedScope` variant and pass no compiler;
//!   the native runtime composition layer owns scope semantics. `open_feed` is
//!   scope-agnostic.
//! - D4: teardown reuses the existing `unregister_feed`,
//!   observed-projection close, and dependent-interest cleanup paths via the
//!   recorded closures — no second feed engine, no re-derived filter on close.
//! - D6: a compiler error is a typed [`FeedOpenError`]; double close is a safe
//!   no-op; poisoned locks fail closed.
//! - D8: a closed session frees its registry entry and drops its teardown
//!   closures, releasing everything the open registered (no leak).

use crate::app_struct::IdentityChangeObserverSlot;
use crate::NmpApp;
use nmp_core::__ffi_internal::SnapshotProjectionSlot;
#[cfg(test)]
use nmp_core::__ffi_internal::{unregister_observer, ObservedProjectionSinkSlot};
#[cfg(test)]
use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::CommandSender;
#[cfg(test)]
use nmp_core::ObservedProjectionId;
use nmp_feed::{
    FeedHandle, FeedParams, FeedRegistrySlot, FeedSessionBuild, FeedSessionId, ProjectionKey,
    TeardownAction,
};
pub use nmp_feed_session::FeedOpenError;

/// The result a [`FeedCompiler`] returns on success: the projection key the
/// session emits under and the ordered teardown recipe that releases everything
/// the compile registered over the existing mechanics.
///
/// This is exactly [`nmp_feed::FeedSessionBuild`]; aliased here so crate-local
/// call sites read as "what the compiler produced".
pub(crate) type FeedCompileOutput = FeedSessionBuild;

/// A scope→registration compiler. `open_feed` invokes it once, AFTER primary-kind
/// validation, to perform the real registration over the existing feed mechanics
/// and return the teardown recipe.
///
/// The compiler MUST register everything the session owns (projection, observer,
/// pull controller, typed sidecar) and return the matching teardown closures; it
/// MUST NOT itself touch the session registry. A scope it does not yet support
/// returns [`FeedOpenError::ScopeNotSupportedYet`] WITHOUT registering anything
/// (fail closed — no partial registration to leak).
pub(crate) trait FeedCompiler {
    /// Compile + register the feed described by `params` against `app`, or fail
    /// closed with a typed error.
    ///
    /// `acquisition_kinds` is the validated, compiled acquisition kind set
    /// (`primary ∪ derived wrappers ∪ kind 5`) that [`NmpApp::open_feed`] already
    /// produced via the single canonical validator — so the compiler does NOT
    /// re-derive or re-validate it. `open_feed` enforces fail-closed primary-kind
    /// validation at this seam BEFORE the compiler runs, so an invalid
    /// declaration can never reach a compiler (the enforcement is not left to
    /// each compiler's good behaviour).
    fn compile(
        &self,
        app: &NmpApp,
        params: &FeedParams,
        acquisition_kinds: &std::collections::BTreeSet<u32>,
    ) -> Result<FeedCompileOutput, FeedOpenError>;
}

/// Blanket impl so a plain closure can be used as a [`FeedCompiler`] — the
/// common case for `explicit composition` / tests that don't need a stateful compiler.
impl<F> FeedCompiler for F
where
    F: Fn(
        &NmpApp,
        &FeedParams,
        &std::collections::BTreeSet<u32>,
    ) -> Result<FeedCompileOutput, FeedOpenError>,
{
    fn compile(
        &self,
        app: &NmpApp,
        params: &FeedParams,
        acquisition_kinds: &std::collections::BTreeSet<u32>,
    ) -> Result<FeedCompileOutput, FeedOpenError> {
        self(app, params, acquisition_kinds)
    }
}

/// `Send` capture of the registry slots a feed-session teardown needs, so a
/// teardown closure can release the session's registrations WITHOUT holding
/// `&NmpApp`.
///
/// Each `teardown_*` method builds a single [`TeardownAction`] that reuses the
/// SAME underlying registry primitive the registration used — there is no second
/// teardown path (D4). A compiler assembles a `Vec<TeardownAction>` from these
/// and returns it in its [`FeedCompileOutput`]; `close_feed` runs them.
#[derive(Clone)]
pub(crate) struct FeedTeardown {
    feeds: FeedRegistrySlot,
    projections: SnapshotProjectionSlot,
    #[cfg(test)]
    observers: ObservedProjectionSinkSlot,
    #[cfg(test)]
    observed_projection_sessions: Option<
        std::sync::Arc<
            std::sync::Mutex<
                std::collections::HashMap<
                    ObservedProjectionId,
                    (String, String, u32, Option<String>),
                >,
            >,
        >,
    >,
    identity_observers: Option<IdentityChangeObserverSlot>,
    sender: CommandSender,
}

impl FeedTeardown {
    /// Build a teardown handle from an `NmpApp`'s registry slots (clones the
    /// `Arc`s + the cheap command sender — captures nothing borrowed).
    #[must_use]
    pub(crate) fn for_app(app: &NmpApp) -> Self {
        Self {
            feeds: app.feed_registry_handle(),
            projections: app.snapshot_projections_handle(),
            #[cfg(test)]
            observers: app.event_observers_handle(),
            #[cfg(test)]
            observed_projection_sessions: Some(app.observed_projection_sessions.clone()),
            identity_observers: Some(app.identity_change_observers.clone()),
            sender: app.command_sender(),
        }
    }

    /// Build a teardown handle from the four registry slots + command sender
    /// directly. [`Self::for_app`] is the production caller; this lower-level
    /// constructor lets a test inject a CAPTURING [`CommandSender`] so it can
    /// observe command-send order relative to the registry removals (#1740 step
    /// 2 teardown-order proof).
    #[must_use]
    #[cfg(test)]
    pub(crate) fn from_parts(
        feeds: FeedRegistrySlot,
        projections: SnapshotProjectionSlot,
        observers: ObservedProjectionSinkSlot,
        sender: CommandSender,
    ) -> Self {
        Self {
            feeds,
            projections,
            observers,
            observed_projection_sessions: None,
            identity_observers: None,
            sender,
        }
    }

    /// A teardown step that drops the feed controller registered under `key`
    /// (reuses [`nmp_feed::FeedRegistry::unregister`]).
    #[must_use]
    pub(crate) fn unregister_feed(&self, key: impl Into<String>) -> TeardownAction {
        let feeds = self.feeds.clone();
        let key = key.into();
        Box::new(move || {
            let _ = feeds.unregister(&key);
        })
    }

    /// A teardown step that removes the (generic + typed) snapshot projection
    /// registered under `key` AND its STRUCTURALLY-PAIRED feed-author provider
    /// (ADR-0063 D7, #1671 Lane H, #1740).
    ///
    /// A session feed installs its typed sidecar through
    /// `register_feed_render_source`, which pairs a feed-author auto-resolve
    /// provider under the same key (so the session's rendered authors resolve
    /// avatars). That provider lives in the same snapshot registry as the typed
    /// projection, so closing the session must drop BOTH in the same lock — exactly
    /// the canonical `NmpApp::unregister_feed` ordering. Without removing the
    /// provider here it would leak: the kernel's next in-tick reconcile would keep
    /// the consumer in the live set and never release the refs it auto-resolved.
    #[must_use]
    pub(crate) fn remove_projection(&self, key: impl Into<String>) -> TeardownAction {
        let projections = self.projections.clone();
        let key = key.into();
        Box::new(move || {
            if let Ok(mut registry) = projections.lock() {
                let removed_proj = registry.remove(&key);
                let removed_provider = registry.remove_feed_author_provider(&key);
                let _ = removed_proj || removed_provider;
            }
        })
    }

    /// A teardown step that closes the observed projection `id`.
    ///
    /// Production feed sessions use ids returned by
    /// `ObservedProjectionRegistrar::open_observed_projection`; closing them
    /// must withdraw the paired interest and unregister the sink. Lower-level
    /// tests built via [`Self::from_parts`] have no session map, so they fall
    /// back to raw sink unregister.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn revoke_observer(&self, id: ObservedProjectionId) -> TeardownAction {
        let observers = self.observers.clone();
        let sessions = self.observed_projection_sessions.clone();
        let sender = self.sender.clone();
        Box::new(move || {
            if let Some(sessions) = &sessions {
                let params = sessions
                    .lock()
                    .ok()
                    .and_then(|mut sessions| sessions.remove(&id));
                if let Some((filter_json, consumer_id, scope, relay_pin)) = params {
                    let _ = sender.send(ActorCommand::Interests(InterestsCommand::CloseInterest {
                        filter_json,
                        consumer_id,
                        scope,
                        relay_pin,
                    }));
                }
            }
            unregister_observer(&observers, id);
        })
    }

    /// A teardown step that revokes a session-scoped active-account observer.
    ///
    /// App-level runtimes may register identity observers for the app lifetime.
    /// Feed sessions are shorter-lived, so reduced-source reset hooks must be
    /// removed on close just like observed-projection sinks and acquisition sets.
    #[must_use]
    pub(crate) fn revoke_identity_observer(
        &self,
        id: crate::IdentityChangeObserverId,
    ) -> TeardownAction {
        let observers = self.identity_observers.clone();
        Box::new(move || {
            if let Some(observers) = observers.as_ref() {
                crate::app_struct::unregister_identity_change_observer(observers, id);
            }
        })
    }

    /// A teardown step that posts a `MarkChangedSinceEmit` so the next snapshot
    /// tick reflects the removed registrations. Run last (so it fires after the
    /// removals). A closed inbox is a silent drop (D6).
    #[must_use]
    pub(crate) fn mark_changed(&self) -> TeardownAction {
        let sender = self.sender.clone();
        Box::new(move || {
            sender.mark_changed_since_emit();
        })
    }
}

impl NmpApp {
    /// #1740 step 2 — a [`FeedTeardown`] over this app's registry slots, for a
    /// feed-session compiler to build its teardown recipe from.
    #[must_use]
    pub(crate) fn feed_teardown(&self) -> FeedTeardown {
        FeedTeardown::for_app(self)
    }

    /// #1740 step 2 — open ONE feed session owning its full lifecycle.
    ///
    /// 1. Validate `params`' primary kinds at THIS seam (fail-closed on
    ///    wrapper/delete/empty), deriving the acquisition kind set. The validator
    ///    (`validate_feed_params`) is the single canonical owner of that protocol
    ///    knowledge; this seam names no wrapper/delete kind itself.
    /// 2. Run the canonical NMP feed compiler to register the feed over the
    ///    EXISTING mechanics and produce its teardown recipe (or fail closed for
    ///    an unsupported scope).
    /// 3. Record the recipe in the session registry under a freshly minted id.
    /// 4. Return a [`FeedHandle`] pairing the projection key with the session id.
    ///
    /// The returned handle is the ONLY thing [`Self::close_feed`] needs — close
    /// never re-derives a filter from the params (D4). On any failure nothing is
    /// left registered (the compiler fails closed before registering; a registry
    /// failure runs the just-produced teardown immediately).
    pub fn open_feed(&self, params: &FeedParams) -> Result<FeedHandle, FeedOpenError> {
        self.open_feed_with_compiler(params, &nmp_feed_session::compile_feed_params)
    }

    /// Internal/test/composition seam for callers that need to inject a compiler.
    ///
    /// Product and facade code should use [`Self::open_feed`], which applies the
    /// canonical NMP compiler implicitly. Keeping this method separately named
    /// prevents compiler selection from being taught as the normal app-facing feed
    /// lifecycle.
    #[doc(hidden)]
    pub(crate) fn open_feed_with_compiler(
        &self,
        params: &FeedParams,
        compiler: &impl FeedCompiler,
    ) -> Result<FeedHandle, FeedOpenError> {
        self.open_feed_with_output(params, |app, params, acquisition_kinds| {
            compiler
                .compile(app, params, acquisition_kinds)
                .map(|build| (build, ()))
        })
        .map(|(handle, ())| handle)
    }

    pub(crate) fn open_feed_with_output<T>(
        &self,
        params: &FeedParams,
        compile: impl FnOnce(
            &NmpApp,
            &FeedParams,
            &std::collections::BTreeSet<u32>,
        ) -> Result<(FeedCompileOutput, T), FeedOpenError>,
    ) -> Result<(FeedHandle, T), FeedOpenError> {
        // 1. Fail-closed primary-kind validation, ENFORCED at the seam (not left
        //    to each compiler). The single canonical validator rejects wrapper/
        //    delete/empty primary kinds and derives the acquisition kind set.
        let acquisition_kinds =
            crate::validate_feed_params(params).map_err(FeedOpenError::InvalidParams)?;

        // 2. Compile + register over the existing mechanics. A scope not yet
        //    wired fails closed here WITHOUT having registered anything.
        let (build, output) = compile(self, params, &acquisition_kinds)?;
        let projection_key = build.projection_key.clone();

        // 3. Record the teardown recipe; mint the session id.
        let session_id = self.feed_sessions.open(build);
        if session_id == FeedSessionId(0) {
            // Registry poisoned: `open` already ran teardown, nothing leaked.
            return Err(FeedOpenError::RegistryUnavailable);
        }

        // 4. Hand back the handle the app addresses the session by.
        Ok((
            FeedHandle {
                projection_key,
                session_id,
            },
            output,
        ))
    }

    /// #1740 step 2 — tear down a session opened by [`Self::open_feed`], using
    /// the HANDLE (not a re-derived filter).
    ///
    /// Looks the session up by `handle.session_id` and runs its recorded
    /// teardown — observer revoke, projection removal, pull-controller /
    /// interest teardown — exactly once, in reverse registration order. Returns
    /// `true` when a live session was torn down.
    ///
    /// Idempotent (D6): closing a handle whose session is already closed (or was
    /// never opened) is a harmless no-op returning `false`. The session entry is
    /// removed from the registry, so its resources are released (D8 — no leak),
    /// proven by the registry no longer reporting the id live.
    pub fn close_feed(&self, handle: &FeedHandle) -> bool {
        self.feed_sessions.close(&handle.session_id)
    }

    /// Test/diagnostic — whether the session behind `handle` is currently live.
    #[must_use]
    pub fn feed_session_is_open(&self, handle: &FeedHandle) -> bool {
        self.feed_sessions.is_open(&handle.session_id)
    }

    /// Test/diagnostic — count of live feed sessions (proves teardown frees the
    /// registry entry rather than flipping a flag).
    #[must_use]
    pub fn live_feed_session_count(&self) -> usize {
        self.feed_sessions.live_count()
    }

    /// #1740 step 4 — register a CLOSED-DATA custom-perspective definition under
    /// an opaque id, for a Rust app crate to declare app-defined
    /// admission/ranking WITHOUT a `Perspective` trait or a native closure
    /// crossing FFI.
    ///
    /// `def` is pure data — a [`nmp_feed::FeedScope`] acquisition + a
    /// [`nmp_feed::FeedRanking`]. After registration a [`FeedParams`] may
    /// reference `id` via `FeedScope::CustomPerspectiveId(id)` (acquisition),
    /// `FeedAdmission::Custom(id)` (admission gate), or `FeedRanking::Custom(id)`
    /// (ranking); the perspective compiler resolves the id back to this
    /// definition and compiles it through the SAME step-3 resolver. An
    /// UNREGISTERED id still fails closed at open.
    ///
    /// Register-ONCE: returns `true` if `id` was newly registered, `false` if it
    /// was already registered (the EXISTING definition stands — see below) or the
    /// registry lock is poisoned. Definitions are IMMUTABLE and not individually
    /// retractable; the registry lives for the life of the app (process-lifetime
    /// in practice).
    ///
    /// Immutability is a fail-CLOSED safety property: a live feed session
    /// captured the COMPILED admission of the definition that existed when it
    /// opened. Allowing an overwrite to a narrower gate would leave already-open
    /// feeds admitting under the stale WIDER policy — a fail-OPEN leak. So a
    /// definition never changes underneath a running session.
    ///
    /// FFI note: this is the Rust-side registration only. A C-ABI / wasm
    /// surface for app-defined perspectives is deferred to a later #1740 step;
    /// the closed-data definition is exactly what such a surface would carry, so
    /// no closure ever needs to cross the boundary.
    pub fn register_custom_perspective(
        &self,
        id: nmp_feed::CustomPerspectiveId,
        def: nmp_feed::CustomPerspectiveDef,
    ) -> bool {
        self.custom_perspectives.register(id, def)
    }

    /// #1740 step 4 — the definition registered under `id`, or `None` if
    /// unregistered. The perspective compiler keys on `None` to fail closed.
    #[must_use]
    pub fn custom_perspective(
        &self,
        id: &nmp_feed::CustomPerspectiveId,
    ) -> Option<nmp_feed::CustomPerspectiveDef> {
        self.custom_perspectives.get(id)
    }

    /// Test/diagnostic — count of registered custom perspectives.
    #[must_use]
    pub fn custom_perspective_count(&self) -> usize {
        self.custom_perspectives.len()
    }
}

/// Convenience: the projection key a session emits under, for callers that hold
/// only a [`FeedHandle`].
#[must_use]
pub fn handle_projection_key(handle: &FeedHandle) -> &ProjectionKey {
    &handle.projection_key
}

#[cfg(test)]
#[path = "feed_session_tests.rs"]
mod tests;
