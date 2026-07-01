//! The generalized session-engine builder for feed scopes (#1740 step 3).
//!
//! Every [`nmp_feed::FeedScope`] compiles through here. The builder is a session
//! wrapper over the existing OP-feed mechanics, parameterized on:
//!
//! * a COMPILED, EVENT-AWARE admission predicate (the engine's
//!   [`nmp_feed::RootAdmission`], built from resolved
//!   pubkey-set DATA / `#t` tag terms — no app closure crosses the seam) that
//!   gates which roots ENTER the feed (#1740 step 3); and
//! * a set of INTERNAL acquisition interests, registered via the kernel's
//!   dependent-interest owner under the session's projection key and withdrawn
//!   symmetrically on close.
//!
//! The session registers under the caller's UNIQUE [`nmp_feed::ProjectionKey`],
//! so many scope sessions coexist. Close
//! tears everything down in reverse order: withdraw each interest, remove the
//! controller + projection, revoke the ingest observer + any resolver observers.
//!
//! Doctrine map:
//! - D0: this names no app product — it consumes a compiled predicate + typed
//!   acquisition shapes. The scope→predicate semantics live in `resolve.rs`.
//! - D4: reuses `register_op_feed` + `op_feed_observer` + the kernel
//!   dependent-interest owner; no second feed engine, no re-derived filter on
//!   close.
//! - D8: each session's interests are withdrawn on close (symmetric teardown).

use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    ClosureInterestShapes, FeedAdvance, FeedApply, FeedController, FeedRender, FeedReset,
    FeedSessionBuild, PullFeedController, RootAdmission,
};
use nmp_note_feed::OpFeedEngine;
use nmp_planner::InterestScope;

use super::source::{OpSessionIdentity, ReducedSource};
use super::trellis_adapter::FeedSessionTrellisAdapter;

mod flat_session;

pub struct ScopeSessionBuild {
    pub build: FeedSessionBuild,
    pub artifacts: Option<OpScopeSessionArtifacts>,
}

pub struct OpScopeSessionArtifacts {
    pub engine: Arc<OpFeedEngine>,
    pub controller: Arc<dyn FeedController>,
    pub follow_set: Option<Arc<nmp_nip02::ActiveFollowSet>>,
}

/// Build a registered feed session for a reduced non-default source and return
/// its teardown recipe.
///
/// `key` is the session's unique projection key (from `params.projection`).
/// `resolved` carries the compiled admission predicate, fixed + live
/// acquisition interests, reset hooks, and any resolver observer ids that must
/// be revoked on close.
pub(super) fn build_scope_session_with_artifacts(
    app: &impl FeedSessionHost,
    key: &str,
    render: &FeedRender,
    resolved: ReducedSource,
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<ScopeSessionBuild, FeedOpenError> {
    match render {
        FeedRender::OpCentric => build_op_scope_session(app, key, resolved, suppression),
        FeedRender::Flat => {
            flat_session::build_flat_scope_session(app, key, resolved).map(|build| {
                ScopeSessionBuild {
                    build,
                    artifacts: None,
                }
            })
        }
    }
}

fn build_op_scope_session(
    app: &impl FeedSessionHost,
    key: &str,
    resolved: ReducedSource,
    suppression: Arc<dyn SuppressionLookup>,
) -> Result<ScopeSessionBuild, FeedOpenError> {
    let ReducedSource {
        op_session_identity,
        admission,
        attribution,
        interests,
        live_shape: _,
        live_shapes,
        observer_scope,
        extra_acquisition,
        reset_hooks,
        source_effect_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set,
    } = resolved;

    let viewer = match (
        crate::read_active(&app.active_account_handle()),
        op_session_identity,
    ) {
        (Some(viewer), _) => viewer,
        (None, OpSessionIdentity::AllowMissingActive) => String::new(),
        (None, OpSessionIdentity::RequireActive) => {
            return Err(FeedOpenError::ScopeNotSupportedYet {
                scope: "scope-no-active-account",
            });
        }
    };

    // ── 1. Engine over separate root-admission and attribution predicates ──
    //
    // Root admission gates events that may enter the feed as roots. Attribution
    // gates authors whose replies/reposts may surface or annotate roots. These
    // are related but not the same: an admitted attribution can surface a root
    // whose author is not directly admitted by the source set.
    let root_admission: RootAdmission = admission;
    let follow_predicate = attribution;
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &nmp_core::substrate::EventId| {
        nmp_core::slots::event_by_id_from_store(&event_store, id)
    });
    let event_lookup_for_observer = event_lookup.clone();
    let engine = nmp_note_feed::op_feed::register_op_feed_with_admission(
        viewer,
        follow_predicate,
        root_admission,
        event_lookup,
    );

    // ── 2. Ingest observer ───────────────────────────────────────────────
    let observer = nmp_note_feed::op_feed::op_feed_observer(
        engine.clone(),
        event_lookup_for_observer,
        suppression,
    );
    let observer_for_registry: Arc<dyn ObservedProjectionSink> = observer.clone();
    let engine_observer = crate::dynamic_observer::DynamicObservedProjectionSet::new(
        app.observed_projection_handle(),
        observer_for_registry,
        format!("{key}.observer"),
        interest_scope_code(observer_scope.clone()),
        live_shapes.clone(),
        512,
    );
    engine_observer.sync();

    // ── 3. Pull controller over the live acquisition shape ───────────────
    let provider: Arc<dyn nmp_feed::FeedInterestShapes + Send + Sync> = {
        let live_shapes = live_shapes.clone();
        Arc::new(ClosureInterestShapes::new(move || live_shapes()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let observer = Arc::clone(&observer);
        let engine = Arc::clone(&engine);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_payload(&engine);
            observer.on_kernel_event(event);
            visible_payload(&engine) != before
        })
    };
    let advance: FeedAdvance = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            engine.grow_visible_window();
        })
    };
    let reset: FeedReset = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            let had_rows = !engine.snapshot_current_window().cards.is_empty();
            engine.reset_for_perspective_change();
            had_rows
        })
    };
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_shape_set(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(key.to_string(), controller.clone());

    // ── 3b. Typed NNFS sidecar + feed-author auto-resolve provider, STRUCTURALLY
    //         PAIRED under the session key (ADR-0063 D7, #1671 Lane H, #1740) ────
    //
    // Emits the NNFS typed projection so an `NNFS`-aware host renders the
    // session's window from the typed payload (generic `Value` fallback for
    // others). Sessions emit always (no incremental-apply omit bookkeeping).
    //
    // CRITICAL (the #1740 unblocker): route BOTH lanes through ONE
    // `FeedRenderSource` via `register_feed_render_source` — NOT the bare
    // `register_typed_snapshot_projection`. The bare path installs ONLY the typed
    // sidecar and NO feed-author provider, so the authors a session feed renders
    // would never auto-`resolve_ref` → blank avatars (the exact #1671 coverage
    // hole). Pairing here makes the provider STRUCTURAL for EVERY session-engine
    // scope (Authors/Tag/List/Wot/…): there is no path that installs the sidecar
    // without the provider, so migrating a Chirp feed onto `open_feed` cannot
    // regress avatars. The same per-tick window materialization feeds both lanes,
    // so authors-resolved == window-emitted even if a concurrent `load_older`
    // widens the window mid-tick (no 1-frame blank gap; ADR-0038). Teardown via
    // `remove_projection` / `unregister_feed` (same key) covers both lanes.
    let engine_for_typed = Arc::clone(&engine);
    let typed_key = key.to_string();
    let source =
        nmp_feed::FeedRenderSource::new(move || engine_for_typed.snapshot_current_window());
    app.register_feed_render_source(key.to_string(), source, move |snapshot| {
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_note_feed::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_note_feed::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                nmp_note_feed::op_feed::OP_FEED_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: nmp_note_feed::op_feed::encode_op_feed_snapshot(snapshot),
            ..Default::default()
        })
    });

    // ── 4+5. Trellis-backed dependent acquisition set + reactive re-sync ───
    //
    // The session owns ONE dependent-interest set keyed by its projection. Fixed
    // seed/list interests and live member timeline interests are replaced
    // together. When a source set shrinks, the kernel withdraws disappeared
    // children immediately; teardown sends the empty set. The session never
    // serializes shapes to NIP-01 JSON or tracks a private open log.
    let sender = app.command_sender();
    let acquisition_adapter =
        FeedSessionTrellisAdapter::new(key, FeedRender::OpCentric, interests, sender)?;
    acquisition_adapter.sync(&extra_acquisition, "feed-session-acquisition");

    // Wire each projection-set change to re-sync acquisition for the new
    // members, then reset the window/cursor. Graph-backed sources use the
    // source-effect hook path below.
    for hook in reset_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let acquisition_adapter = acquisition_adapter.clone();
        let sync_observer = engine_observer.clone();
        let reset_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_observer.sync();
            acquisition_adapter.sync(&extra, "feed-session-acquisition");
            acquisition_adapter.rebaseline_output_if_changed(controller_for_reset.reset());
        });
        hook(reset_trigger);
    }

    for hook in source_effect_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let acquisition_adapter = acquisition_adapter.clone();
        let sync_observer = engine_observer.clone();
        let source_effect_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_observer.sync();
            acquisition_adapter.sync(&extra, "feed-session-acquisition");
            acquisition_adapter.rebaseline_output_if_changed(controller_for_reset.reset());
        });
        hook(source_effect_trigger);
    }

    // ── 6. Teardown recipe (registration order = reverse of execution) ───
    //
    // Execution order on close (the registry reverses the Vec):
    //   1. unregister the controller            (registry removal, runs first)
    //   2. revoke the engine ingest observer
    //   3. revoke each resolver observer
    //   4. close Trellis session scope          (clear output + WITHDRAW actor state)
    //   5. mark-changed                         (the notification, runs last)
    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(app.mark_changed_action()); // exec #5 (last)
    teardown.push(acquisition_adapter.close_action(app.remove_projection_action(key.to_string()))); // exec #4
    for id in resolver_observer_ids {
        let handle = app.observed_projection_handle();
        teardown.push(Box::new(move || handle.close(id)));
    } // exec #3
    for id in identity_observer_ids {
        teardown.push(app.unregister_identity_change_observer_action(id));
    } // exec #3
    teardown.extend(resolver_teardown);
    teardown.push(engine_observer.teardown_action()); // exec #2
    teardown.push(app.unregister_feed_action(key.to_string())); // exec #1 (first)

    Ok(ScopeSessionBuild {
        build: FeedSessionBuild {
            projection_key: nmp_feed::ProjectionKey::app_owned(key).unwrap(),
            teardown,
        },
        artifacts: Some(OpScopeSessionArtifacts {
            engine,
            controller,
            follow_set: active_follow_set,
        }),
    })
}

fn visible_payload(engine: &nmp_note_feed::OpFeedEngine) -> Vec<u8> {
    let snapshot = engine.snapshot_current_window();
    nmp_note_feed::op_feed::encode_op_feed_snapshot(&snapshot)
}

pub(super) fn visible_flat_payload(feed: &nmp_note_feed::FlatFeed) -> Vec<u8> {
    let snapshot = feed.snapshot_current_window();
    nmp_note_feed::op_feed::encode_op_feed_snapshot(&snapshot)
}

pub(crate) fn interest_scope_code(scope: InterestScope) -> u32 {
    match scope {
        InterestScope::ActiveAccount | InterestScope::Account(_) => 0,
        InterestScope::Global => 1,
    }
}
