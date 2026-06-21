//! `NmpApp::open_feed` / `close_feed` — the feed-session registry seam (#1740
//! step 2).
//!
//! ONE [`NmpApp::open_feed`] call owns a feed's full lifecycle: it mints a
//! session id + projection key, drives a caller-supplied [`FeedCompiler`] to
//! perform the actual registration over the EXISTING feed mechanics
//! (`register_op_feed_defaults` etc.), records the resulting teardown recipe in
//! the engine-agnostic [`nmp_feed::FeedSessionRegistry`], and returns a
//! [`nmp_feed::FeedHandle`]. [`NmpApp::close_feed`] looks the session up by the
//! handle's id and tears it ALL down — observer, projection, pull controller,
//! interests — idempotently, using the HANDLE (never a re-derived filter).
//!
//! Why a compiler closure rather than matching on `FeedScope` here: the
//! concrete wiring of a scope names the OP-feed engine / follow set / typed
//! sidecar, which live in `nmp-defaults` (above `nmp-ffi` in the DAG). Keeping
//! the scope→registration compile in the caller keeps `nmp-ffi` D0-clean (it
//! names no NIP/feed-kind noun) and keeps a single source of truth for feed
//! state: `open_feed` owns only the session bookkeeping, never a second feed
//! engine (D4).
//!
//! Doctrine map:
//! - D0: `nmp-ffi` matches on no `FeedScope` variant; the compiler (in the
//!   composition layer) owns scope semantics. `open_feed` is scope-agnostic.
//! - D4: teardown reuses the existing `unregister_feed` / `clear_active_follows`
//!   / `unregister_event_observer` paths via the recorded closures — no second
//!   feed engine, no re-derived filter on close.
//! - D6: a compiler error is a typed [`FeedOpenError`]; double close is a safe
//!   no-op; poisoned locks fail closed.
//! - D8: a closed session frees its registry entry and drops its teardown
//!   closures, releasing everything the open registered (no leak).

use crate::NmpApp;
use nmp_core::__ffi_internal::{
    unregister_observer, KernelEventObserverSlot, SnapshotProjectionSlot,
};
use nmp_core::{ActorCommand, CommandSender, KernelEventObserverId};
use nmp_feed::{
    FeedHandle, FeedParams, FeedRegistrySlot, FeedSessionBuild, FeedSessionId, ProjectionKey,
    TeardownAction,
};

/// Typed failure of [`NmpApp::open_feed`] (D6 — no panic across the seam).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedOpenError {
    /// The declared [`FeedParams`] failed primary-kind validation (wrapper /
    /// delete / empty primary kinds). Carries the underlying typed error.
    InvalidParams(nmp_feed::FeedParamsError),
    /// The declared [`nmp_feed::FeedScope`] is recognised by the model but not
    /// yet wired by this step. Step 3 (the full perspective compiler) lands the
    /// remaining variants; until then they fail closed with this typed error
    /// rather than silently registering nothing. `scope` is a short stable
    /// machine token naming the unsupported variant (e.g. `"ListMembers"`).
    ScopeNotSupportedYet { scope: &'static str },
    /// The compiler attempted the registration but the session registry could
    /// not track it (a poisoned lock); the compile's teardown has already run,
    /// so nothing leaked. The open is reported as failed.
    RegistryUnavailable,
}

/// The result a [`FeedCompiler`] returns on success: the projection key the
/// session emits under and the ordered teardown recipe that releases everything
/// the compile registered over the existing mechanics.
///
/// This is exactly [`nmp_feed::FeedSessionBuild`]; re-exported here under a
/// task-local alias so call sites read as "what the compiler produced".
pub type FeedCompileOutput = FeedSessionBuild;

/// A scope→registration compiler. `open_feed` invokes it once, AFTER primary-kind
/// validation, to perform the real registration over the existing feed mechanics
/// and return the teardown recipe.
///
/// The compiler MUST register everything the session owns (projection, observer,
/// pull controller, typed sidecar) and return the matching teardown closures; it
/// MUST NOT itself touch the session registry. A scope it does not yet support
/// returns [`FeedOpenError::ScopeNotSupportedYet`] WITHOUT registering anything
/// (fail closed — no partial registration to leak).
pub trait FeedCompiler {
    /// Compile + register the feed described by `params` against `app`, or fail
    /// closed with a typed error. The compiled acquisition kind set is passed
    /// pre-validated so the compiler does not re-derive it.
    fn compile(
        &self,
        app: &NmpApp,
        params: &FeedParams,
        acquisition_kinds: &std::collections::BTreeSet<u32>,
    ) -> Result<FeedCompileOutput, FeedOpenError>;
}

/// Blanket impl so a plain closure can be used as a [`FeedCompiler`] — the
/// common case for `nmp-defaults` / tests that don't need a stateful compiler.
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
pub struct FeedTeardown {
    feeds: FeedRegistrySlot,
    projections: SnapshotProjectionSlot,
    observers: KernelEventObserverSlot,
    sender: CommandSender,
}

impl FeedTeardown {
    /// Build a teardown handle from an `NmpApp`'s registry slots (clones the
    /// `Arc`s + the cheap command sender — captures nothing borrowed).
    #[must_use]
    pub fn for_app(app: &NmpApp) -> Self {
        Self::from_parts(
            app.feed_registry_handle(),
            app.snapshot_projections_handle(),
            app.event_observers_handle(),
            app.command_sender(),
        )
    }

    /// Build a teardown handle from the four registry slots + command sender
    /// directly. [`Self::for_app`] is the production caller; this lower-level
    /// constructor lets a test inject a CAPTURING [`CommandSender`] so it can
    /// observe the ORDER in which a recipe posts `ClearActiveFollowsFeed` /
    /// `MarkChangedSinceEmit` relative to the registry removals (#1740 step 2
    /// teardown-order proof).
    #[must_use]
    pub fn from_parts(
        feeds: FeedRegistrySlot,
        projections: SnapshotProjectionSlot,
        observers: KernelEventObserverSlot,
        sender: CommandSender,
    ) -> Self {
        Self {
            feeds,
            projections,
            observers,
            sender,
        }
    }

    /// A teardown step that drops the feed controller registered under `key`
    /// (reuses [`nmp_feed::FeedRegistry::unregister`]).
    #[must_use]
    pub fn unregister_feed(&self, key: impl Into<String>) -> TeardownAction {
        let feeds = self.feeds.clone();
        let key = key.into();
        Box::new(move || {
            let _ = feeds.unregister(&key);
        })
    }

    /// A teardown step that removes the (generic + typed) snapshot projection
    /// registered under `key` (reuses the snapshot registry's `remove`).
    #[must_use]
    pub fn remove_projection(&self, key: impl Into<String>) -> TeardownAction {
        let projections = self.projections.clone();
        let key = key.into();
        Box::new(move || {
            if let Ok(mut registry) = projections.lock() {
                let _ = registry.remove(&key);
            }
        })
    }

    /// A teardown step that revokes the ingest observer `id` (reuses the
    /// kernel-event observer registry's `unregister_observer`). Idempotent for
    /// an unknown id.
    #[must_use]
    pub fn revoke_observer(&self, id: KernelEventObserverId) -> TeardownAction {
        let observers = self.observers.clone();
        Box::new(move || {
            unregister_observer(&observers, id);
        })
    }

    /// A teardown step that posts a `MarkChangedSinceEmit` so the next snapshot
    /// tick reflects the removed registrations. Run last (so it fires after the
    /// removals). A closed inbox is a silent drop (D6).
    #[must_use]
    pub fn mark_changed(&self) -> TeardownAction {
        let sender = self.sender.clone();
        Box::new(move || {
            let _ = sender.send(ActorCommand::MarkChangedSinceEmit);
        })
    }

    /// A teardown step that WITHDRAWS the actor-owned active-follows feed
    /// interests and clears the active-follows internal state.
    ///
    /// This is the close-side of the OPEN-side `declare_active_follows_feed`
    /// that `register_op_feed_defaults` issues (it sends
    /// `ActorCommand::DeclareActiveFollowsFeed`). The open path declares the
    /// follow-feed acquisition kinds, which registers M2 follow-feed interests
    /// against the active account; revoking observers + unregistering the feed
    /// controller does NOT release those actor-owned interests — only
    /// `ActorCommand::ClearActiveFollowsFeed` does (it drives
    /// `kernel.set_follow_feed_kinds(empty)` →
    /// `sync_follow_feed_interests(&[])`, withdrawing every follow-feed
    /// interest, resetting `timeline_authors`, and emitting the CLOSE diff on
    /// the next idle tick). Symmetric: open declared it → close clears it.
    ///
    /// A closed inbox is a silent drop (D6). Idempotent: clearing an already
    /// empty follow-feed is a no-op in the kernel.
    #[must_use]
    pub fn clear_active_follows(&self) -> TeardownAction {
        let sender = self.sender.clone();
        Box::new(move || {
            let _ = sender.send(ActorCommand::ClearActiveFollowsFeed);
        })
    }

    /// #1740 step 3 — a teardown step that WITHDRAWS one session-scoped
    /// acquisition interest opened via `ActorCommand::OpenInterest`.
    ///
    /// This is the close-side of a non-default scope's acquisition (the
    /// perspective compiler's `ContactList` / `ListMembers` / `Wot` / `Tag` /
    /// set-algebra arms register their internal interests with
    /// `ActorCommand::OpenInterest { filter_json, consumer_id, scope }`, the
    /// `consumer_id` being the session's projection key). Closing the same
    /// triple detaches that owner; the kernel reconstructs the same registry
    /// slot from the `InterestShape` hash, so the `(filter_json, consumer_id,
    /// scope)` MUST match the open call. When the last owner of an interest
    /// leaves, the kernel enqueues the CLOSE diff (D8 — bounded: the session's
    /// interests are withdrawn on close).
    ///
    /// A closed inbox is a silent drop (D6); closing an interest that is not
    /// open is a harmless no-op.
    #[must_use]
    pub fn close_interest(
        &self,
        filter_json: impl Into<String>,
        consumer_id: impl Into<String>,
        scope: u32,
    ) -> TeardownAction {
        let sender = self.sender.clone();
        let filter_json = filter_json.into();
        let consumer_id = consumer_id.into();
        Box::new(move || {
            let _ = sender.send(ActorCommand::CloseInterest {
                filter_json,
                consumer_id,
                scope,
            });
        })
    }
}

impl NmpApp {
    /// #1740 step 2 — a [`FeedTeardown`] over this app's registry slots, for a
    /// feed-session compiler to build its teardown recipe from.
    #[must_use]
    pub fn feed_teardown(&self) -> FeedTeardown {
        FeedTeardown::for_app(self)
    }

    /// #1740 step 2 — open ONE feed session owning its full lifecycle.
    ///
    /// 1. Validate `params`' primary kinds (fail-closed on wrapper/delete/empty).
    /// 2. Run `compiler` to register the feed over the EXISTING mechanics and
    ///    produce its teardown recipe (or fail closed for an unsupported scope).
    /// 3. Record the recipe in the session registry under a freshly minted id.
    /// 4. Return a [`FeedHandle`] pairing the projection key with the session id.
    ///
    /// The returned handle is the ONLY thing [`Self::close_feed`] needs — close
    /// never re-derives a filter from the params (D4). On any failure nothing is
    /// left registered (the compiler fails closed before registering; a registry
    /// failure runs the just-produced teardown immediately).
    pub fn open_feed(
        &self,
        params: &FeedParams,
        compiler: &impl FeedCompiler,
    ) -> Result<FeedHandle, FeedOpenError> {
        // 1. Fail-closed primary-kind validation (single home — params.rs).
        let acquisition_kinds = params
            .validate_primary_kinds()
            .map_err(FeedOpenError::InvalidParams)?;

        // 2. Compile + register over the existing mechanics. A scope not yet
        //    wired fails closed here WITHOUT having registered anything.
        let build = compiler.compile(self, params, &acquisition_kinds)?;
        let projection_key = build.projection_key.clone();

        // 3. Record the teardown recipe; mint the session id.
        let session_id = self.feed_sessions.open(build);
        if session_id == FeedSessionId(0) {
            // Registry poisoned: `open` already ran teardown, nothing leaked.
            return Err(FeedOpenError::RegistryUnavailable);
        }

        // 4. Hand back the handle the app addresses the session by.
        Ok(FeedHandle {
            projection_key,
            session_id,
        })
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
