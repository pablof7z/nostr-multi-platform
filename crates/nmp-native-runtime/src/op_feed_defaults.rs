//! `register_op_feed_defaults` — the V-80 rung 6 (Stage 5) composition root
//! that wires the OP-centric feed renderer together.
//!
//! This is the one place in the system that names `NmpApp` (native runtime) and the
//! NIP-10 OP-feed instance (`nmp-nip01`) in the same breath. Every lower layer
//! deliberately avoids that edge: `nmp-feed` is generic, `nmp-nip01`'s
//! `register_op_feed` returns an `Arc<OpFeedEngine>` for *someone else* to
//! register, and `nmp-nip02`'s `ActiveFollowSet` takes an
//! [`ActiveAccountSlot`], not `&NmpApp`. The composition root closes the loop.
//!
//! # What this function wires
//!
//! 1. Constructs [`nmp_nip02::ActiveFollowSet`] over the kernel's
//!    [`ActiveAccountSlot`] (the producer of the follow predicate).
//! 2. Builds the three inputs `register_op_feed` needs:
//!    * **follow predicate** — `active_follow_set.predicate()` (live view of
//!      the active account's follow set);
//!    * **event lookup** — a synchronous read through the kernel event-store
//!      handle exposed by `NmpApp`; this is a local cache read only, never
//!      acquisition;
//!    * **card builder** — supplied inside `register_op_feed` itself
//!      (`TimelineEventCard::from_event_for_op_feed`).
//! 3. Registers the returned `Arc<OpFeedEngine>` as an
//!    [`ObservedProjectionSink`](nmp_core::ObservedProjectionSink) (ingest) and a
//!    [`FeedController`](nmp_feed::FeedController) under
//!    `"nmp.feed.home"` (output).
//! 4. Registers the `ActiveFollowSet` as its own `ObservedProjectionSink` (so
//!    kind:3 ingest keeps the follow set current — exactly the pattern the
//!    sibling `FollowListProjection` already uses).
//! 5. Registers an `on_change` callback that resets the engine on every
//!    follow-set perspective change.
//! 6. Registers the follow-set notifier on `NmpApp`'s identity-change observer
//!    seam so sign-in, switch, logout, and reset are pushed after the actor has
//!    written the active-account slot.
//!
//! # CRITICAL DECISION — declaration here, no static follow snapshots
//!
//! Feed acquisition is owned by the `open_feed(FeedScope::ActiveUserFollows)`
//! reduced-source path. This composition helper wires only the OP-feed render
//! engine and its live follow predicate; it never declares actor-owned
//! acquisition.
//!
//! # `event_lookup` reads the kernel event store (V-83)
//!
//! The engine's `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent>>` is
//! consulted by the repost L-2/L-5 rebuild paths to read a parent/target/wrapper
//! event the engine has not yet observed but the kernel has already cached:
//!
//! * **L-5** (`OpFeedEngine::ingest_root`): a kind:6 repost wrapper keyed the
//!   target id first (placeholder card); when the target arrives, the engine
//!   re-fetches the **wrapper** via `event_lookup` to rebuild the card from the
//!   `(wrapper, target)` pair so the "reposted by" provenance survives. Without
//!   a real lookup the card rebuilds from `(target, None)` — provenance lost.
//! * **L-2** (`OpFeedEngine::ingest_reply`): a reply points at a repost wrapper;
//!   the engine looks the wrapper up to discover it `supersedes` a different
//!   target and re-keys the attribution onto that target instead of the wrapper.
//!
//! V-83 added [`NmpApp::event_by_id`](crate::NmpApp::event_by_id) over the
//! kernel's published `EventStore` handle (the actor publishes
//! `Kernel::event_store_handle()` into a shared slot right after kernel
//! construction and re-publishes on `Reset`). The closure here
//! captures [`NmpApp::event_store_handle`](crate::NmpApp::event_store_handle)
//! (the slot `Arc`, NOT `&app` — the closure outlives the borrow) and reads
//! through it on every call, so a `Reset` is observed without re-capturing.
//! `EventStore::get_by_id` is a `&self` read; the actor reducer is the sole
//! writer (D4) and the store insert is ordered before the observer fan-out, so
//! a read from a `ObservedProjectionSink` callback (actor thread) sees the
//! just-ingested event without re-entrancy. Before `nmp_app_start` the slot is
//! empty → `None`, which is exactly the prior no-op behaviour (still
//! correctness-preserving: the L-2 fallback re-keys on a later observer arrival
//! and L-5 shows the placeholder until the target lands).
//!
//! # Active-account source of truth
//!
//! `ActiveFollowSet::new` needs the kernel's [`ActiveAccountSlot`].
//! `register_op_feed_defaults` reads it directly from
//! [`NmpApp::active_account_handle`](crate::NmpApp::active_account_handle)
//! so the follow predicate and the identity-change observer share the same
//! app-owned `Arc` the actor writes in `Kernel::set_accounts`.
//!
//! # Perspective changes reset the feed
//!
//! `ActiveFollowSet::on_change` fires on active-account kind:3 replacement,
//! account switch, and logout. All of those are feed-perspective changes: the
//! user has changed who can cause rows to appear. The engine therefore resets
//! immediately instead of letting stale rows D5-evict naturally. Re-population
//! comes from the same ReducedSource acquisition/cache-serve path that
//! materializes the current active-account source.
//!
//! ## The account-change race (rung-4 flagged this)
//!
//! On a switch A → B the actor updates the active-account slot, emits a state
//! frame, and `NmpApp`'s update listener fires its identity observers before
//! forwarding that frame to native. The callback registered here calls
//! `notify_account_changed()`: `ActiveFollowSet` clears the set and re-seeds
//! self-inclusion of B (its follows are still empty — B's kind:3 has not landed
//! yet) and fires `on_change`; this callback sees `B != A`, resets the engine,
//! and records B. When B's kind:3 later ingests, `ActiveFollowSet`'s own
//! observer repopulates the set and fires `on_change` again; the callback
//! resets the empty interim window so the new perspective is populated only by
//! B's qualifying rows. The clear-then-repopulate ordering means the
//! switch-before-kind:3 window never rebuilds against a stale follow set.
//!
//! [`ActiveAccountSlot`]: nmp_core::slots::ActiveAccountSlot

use std::sync::Arc;

use crate::{FeedOpenError, NmpApp};
use nmp_core::substrate::{empty_suppression_lookup, SuppressionLookup};
use nmp_feed::{
    FeedAdmission, FeedController, FeedHandle, FeedParams, FeedRanking, FeedRender, FeedScope,
    FeedWindow, ProjectionKey,
};
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::OpFeedEngine;
use nmp_nip02::ActiveFollowSet;
use nmp_nip51::MuteListProjection;

mod active_shape;
mod dynamic_observer;
#[cfg(test)]
use active_shape::live_active_follows_shape;
use active_shape::read_active;

#[cfg(test)]
use nmp_core::slots::ActiveAccountSlot;

/// What [`register_op_feed_defaults`] hands back to the composition caller.
pub struct OpFeedDefaults {
    /// The ordinary feed-session handle for the default home projection.
    ///
    /// `None` means the typed declaration failed closed before registration
    /// (for example because `primary_feed_kinds` named a derived wrapper kind).
    pub handle: Option<FeedHandle>,
    /// Diagnostic handle to the session-owned OP-feed engine.
    pub engine: Arc<OpFeedEngine>,
    /// Diagnostic handle to the session-owned feed controller.
    pub controller: Arc<dyn FeedController>,
    /// Diagnostic handle to the session-owned active follow-set resolver.
    pub follow_set: Arc<ActiveFollowSet>,
}

struct NoopFeedController;

impl FeedController for NoopFeedController {
    fn load_older(&self) -> bool {
        false
    }
}

/// Wire the OP-centric home feed into `app`.
///
/// Builds the default home [`FeedParams`] and opens it through the ordinary
/// typed feed-session compiler. The session engine owns the OP engine,
/// follow-set resolver, observed projection, pull controller, typed sidecar,
/// and teardown recipe under the default projection key.
///
/// # Ordering
///
/// Like [`crate::register_defaults`], call before `nmp_app_start`: the engine
/// and the follow-set observer must be visible to the kernel when the first
/// event arrives.
pub fn register_op_feed_defaults(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
) -> OpFeedDefaults {
    register_op_feed_defaults_inner(app, viewer, primary_feed_kinds, empty_suppression_lookup())
}

/// Wire the OP-centric home feed with the default NIP-51 mute read model.
///
/// Uses the same `MuteListProjection` installed by [`crate::register_defaults`]
/// and resets the current feed window whenever the active account's mute list
/// replacement changes.
pub fn register_op_feed_defaults_with_mute(
    app: &NmpApp,
    viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    mute: Arc<MuteListProjection>,
) -> OpFeedDefaults {
    let suppression: Arc<dyn SuppressionLookup> = mute.clone();
    let defaults = register_op_feed_defaults_inner(app, viewer, primary_feed_kinds, suppression);
    if defaults.handle.is_some() {
        let registry = app.feed_registry_handle();
        let sender = app.command_sender();
        mute.on_change(Box::new(move || {
            if registry.reset(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY) {
                sender.mark_changed_since_emit();
            }
        }));
    }
    defaults
}

fn register_op_feed_defaults_inner(
    app: &NmpApp,
    _viewer: Pubkey,
    primary_feed_kinds: Vec<u32>,
    suppression: Arc<dyn SuppressionLookup>,
) -> OpFeedDefaults {
    let params = default_home_feed_params(primary_feed_kinds);
    let compiler = move |app: &NmpApp,
                         params: &FeedParams,
                         kinds: &std::collections::BTreeSet<u32>|
          -> Result<
        (
            nmp_feed::FeedSessionBuild,
            session_compile::OpScopeSessionArtifacts,
        ),
        FeedOpenError,
    > {
        let detailed = session_compile::compile_feed_params_with_suppression_and_artifacts(
            app,
            params,
            kinds,
            Arc::clone(&suppression),
        )?;
        let Some(artifacts) = detailed.artifacts else {
            return Err(FeedOpenError::ScopeNotSupportedYet {
                scope: "default-home-feed-artifacts",
            });
        };
        Ok((detailed.build, artifacts))
    };
    match app.open_feed_with_output(&params, compiler) {
        Ok((handle, artifacts)) => OpFeedDefaults {
            handle: Some(handle),
            engine: artifacts.engine,
            controller: artifacts.controller,
            follow_set: artifacts
                .follow_set
                .unwrap_or_else(|| fallback_follow_set(app)),
        },
        Err(_) => fallback_defaults(app),
    }
}

#[must_use]
pub fn default_home_feed_params(primary_feed_kinds: Vec<u32>) -> FeedParams {
    FeedParams {
        primary_kinds: primary_feed_kinds,
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow {
            initial_limit: nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        },
        projection: ProjectionKey(nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY.to_string()),
    }
}

fn fallback_defaults(app: &NmpApp) -> OpFeedDefaults {
    let follow_set = fallback_follow_set(app);
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup =
        Arc::new(move |id| nmp_core::slots::event_by_id_from_store(&event_store, id));
    let engine =
        nmp_nip01::op_feed::register_op_feed(String::new(), follow_set.predicate(), event_lookup);
    OpFeedDefaults {
        handle: None,
        engine,
        controller: Arc::new(NoopFeedController),
        follow_set,
    }
}

fn fallback_follow_set(app: &NmpApp) -> Arc<ActiveFollowSet> {
    ActiveFollowSet::new(app.active_account_handle())
}

// #1740 step 2 — `FeedParams` → existing-registration compiler (sibling module).
mod session_compile;
pub use session_compile::compile_feed_params;

#[cfg(test)]
#[path = "op_feed_defaults/tests.rs"]
mod tests;
