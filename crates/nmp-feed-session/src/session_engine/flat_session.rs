use std::sync::{Arc, Mutex};

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    ClosureInterestShapes, FeedAdvance, FeedApply, FeedController, FeedReset, FeedSessionBuild,
    FeedShape, FeedWindowPolicy, PullFeedController,
};

use super::{interest_scope_code, visible_flat_payload};
use crate::source::ReducedSource;
use crate::trellis_adapter::FeedSessionTrellisAdapter;

/// Build a registered flat-shape feed session.
///
/// `item_builder` and `merge` are the identity + sort/merge knobs (#3082 — the
/// two engine changes: `item_builder` is arity-`Vec`, so it can fan a single
/// event out into zero, one, or many rows). The single-scope `FeedParams` path
/// supplies `nmp_note_feed::feed_row_builder`/`timeline_merge` (unchanged
/// behavior); the composite-lane compiler supplies its own combined
/// lane-dispatching builder/merge over the SAME engine.
pub(super) fn build_flat_scope_session(
    app: &impl FeedSessionHost,
    key: &str,
    window: FeedWindowPolicy,
    resolved: ReducedSource,
    item_builder: nmp_feed::FlatFeedItemBuilder<nmp_feed::FeedRow>,
    merge: nmp_feed::FlatFeedMerge<nmp_feed::FeedRow>,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let ReducedSource {
        op_session_identity: _,
        admission,
        attribution: _,
        interests,
        live_shape: _,
        live_shapes,
        observer_scope,
        extra_acquisition,
        reactivity_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set: _,
        // The single-scope `FeedParams` caller already folded `row_context`
        // into its `item_builder` closure before calling this function; the
        // composite-lane compiler folds its per-lane row contexts the same
        // way. Neither needs it again here.
        row_context: _,
    } = resolved;

    // Generic flat engine over the kind-agnostic `FeedRow`. The four knobs:
    //   admission  = the compiled perspective predicate (`admission`);
    //   identity   = caller-supplied `item_builder` (arity-`Vec`, #3082);
    //   sort/merge = caller-supplied `merge`.
    let feed = nmp_feed::FlatFeed::with_merge_and_window_policy(
        admission,
        item_builder,
        None,
        merge,
        window,
    );
    // Built here (before `observer_for_registry`/`apply`) so BOTH ingestion
    // paths below can re-sync it per event — nothing below depends on
    // `provider`/`pull`/`apply`/`controller` existing first.
    let sender = app.command_sender();
    let acquisition_adapter = FeedSessionTrellisAdapter::new_with_diagnostics(
        key,
        FeedShape::Flat,
        interests.clone(),
        sender,
        app.feed_session_diagnostics(),
    )?;

    // A composite lane's mapping may register NEW `Delivered`-ref demand as a
    // side effect of ingesting an event (#3082/#3086). LIVE events reach this
    // session through the OBSERVED-PROJECTION sink registered right below
    // (`ResyncingObserver`) — NOT through `apply`, which `PullFeedController`
    // uses only for the on-demand `load_older` PULL/backfill drain, a
    // SEPARATE ingestion path (see `pull_controller.rs`'s module doc). Both
    // paths must resync so a newly demanded target actually gets
    // (re-)fetched regardless of which path delivered the demanding event:
    //   1. the OBSERVED-projection `engine_observer`, so an event the kernel
    //      already holds/streams for the new shape reaches `on_kernel_event`;
    //   2. the Trellis-backed `acquisition_adapter`, so a target that is NOT
    //      already flowing (arrived-and-dropped before any lane demanded it,
    //      or never fetched at all) gets a fresh dependent-interest
    //      open/replace and is actually (re-)acquired — without this, a
    //      target-first delivery ordering permanently stalls on a
    //      placeholder row, because the admission gate that dropped the
    //      target's first delivery has no OTHER path back in.
    // `engine_observer` is constructed AFTER this sink (it wraps it as the
    // registered observer), so the sink resolves it through a slot populated
    // once construction below completes.
    let engine_observer_slot: Arc<
        Mutex<Option<crate::dynamic_observer::DynamicObservedProjectionSet>>,
    > = Arc::new(Mutex::new(None));
    let observer_for_registry: Arc<dyn ObservedProjectionSink> = Arc::new(ResyncingObserver {
        feed: Arc::clone(&feed),
        acquisition_adapter: acquisition_adapter.clone(),
        extra_acquisition: extra_acquisition.clone(),
        engine_observer: Arc::clone(&engine_observer_slot),
    });
    let engine_observer = crate::dynamic_observer::DynamicObservedProjectionSet::new(
        app.observed_projection_handle(),
        observer_for_registry,
        format!("{key}.observer"),
        interest_scope_code(observer_scope),
        live_shapes.clone(),
        512,
    );
    *engine_observer_slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(engine_observer.clone());
    engine_observer.sync();

    let provider: Arc<dyn nmp_feed::FeedInterestShapes + Send + Sync> = {
        let live_shapes = live_shapes.clone();
        Arc::new(ClosureInterestShapes::new(move || live_shapes()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let feed = Arc::clone(&feed);
        let engine_observer = engine_observer.clone();
        let acquisition_adapter = acquisition_adapter.clone();
        let extra_acquisition = extra_acquisition.clone();
        Arc::new(move |event: &KernelEvent| {
            let before = visible_flat_payload(&feed);
            feed.on_kernel_event(event);
            // Mirrors `ResyncingObserver` (above) for the PULL/backfill
            // ingestion path (#3082/#3086) — a drained page can ALSO register
            // new `Delivered`-ref demand, and `apply` is this path's own
            // ingest entry point (`pull_controller.rs`'s module doc), so it
            // needs the SAME two re-syncs. Both are no-ops in the common case
            // (shape/demand-map unchanged) — `DynamicObservedProjectionSet::sync`'s
            // `same_shape_set` check and `FeedSessionTrellisAdapter::sync`'s
            // resource-ledger diff each cost one comparison.
            engine_observer.sync();
            acquisition_adapter.sync(&extra_acquisition, "feed-session-acquisition-demand");
            visible_flat_payload(&feed) != before
        })
    };
    let advance: FeedAdvance = {
        let feed = Arc::clone(&feed);
        Arc::new(move || {
            feed.grow_visible_window();
        })
    };
    let reset: FeedReset = {
        let feed = Arc::clone(&feed);
        Arc::new(move || feed.reset_for_perspective_change())
    };
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_shape_set_and_window_policy(
            provider,
            pull,
            apply,
            None,
            Some(reset),
            advance,
            window,
        );
    app.register_feed(key.to_string(), controller.clone());

    let feed_for_typed = Arc::clone(&feed);
    let typed_key = key.to_string();
    let source = nmp_feed::FeedWindowSource::new(move || feed_for_typed.snapshot_current_window());
    app.register_feed_window_source(key.to_string(), source, move |snapshot| {
        // Frozen feed-row FlatBuffers wire (`NFRS`, #3082 settled design).
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_feed::typed_wire::FEED_ROW_SCHEMA_ID.to_string(),
            schema_version: nmp_feed::typed_wire::FEED_ROW_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                nmp_feed::typed_wire::FEED_ROW_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: nmp_feed::typed_wire::encode_feed_row_snapshot(snapshot),
            ..Default::default()
        })
    });
    let replayed_tail = app.load_older_feed(key);
    let replayed_ids = super::super::flat_replay::replay_fixed_event_ids(app, &feed, &interests);
    acquisition_adapter.rebaseline_output_if_changed(replayed_ids && !replayed_tail);

    acquisition_adapter.sync(&extra_acquisition, "feed-session-acquisition");
    for hook in reactivity_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let acquisition_adapter = acquisition_adapter.clone();
        let sync_observer = engine_observer.clone();
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_observer.sync();
            let reset = controller_for_reset.reset();
            let replayed = controller_for_reset.load_older();
            acquisition_adapter.schedule_source_effect(
                Arc::clone(&extra),
                "feed-session-acquisition",
                reset || replayed,
            );
        });
        hook(trigger);
    }

    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(app.mark_changed_action());
    teardown.push(acquisition_adapter.close_action(app.remove_projection_action(key.to_string())));
    for id in resolver_observer_ids {
        let handle = app.observed_projection_handle();
        teardown.push(Box::new(move || handle.close(id)));
    }
    for id in identity_observer_ids {
        teardown.push(app.unregister_identity_change_observer_action(id));
    }
    teardown.extend(resolver_teardown);
    teardown.push(engine_observer.teardown_action());
    teardown.push(app.unregister_feed_action(key.to_string()));

    Ok(FeedSessionBuild {
        projection_key: nmp_feed::ProjectionKey::app_owned(key).unwrap(),
        teardown,
    })
}

/// The sink registered with the kernel's observed-projection dispatch for
/// LIVE (push) event delivery (#3082/#3086) — replaces a bare `feed.clone()`
/// so a `Delivered`-ref demand a lane registers while ingesting one event
/// re-syncs BOTH the observed-projection shape and the Trellis acquisition
/// before the next event arrives. See the constructor's doc comment in
/// `build_flat_scope_session` for why this is a SEPARATE mechanism from
/// `apply`'s equivalent resync (`apply` is the PULL/backfill path only).
struct ResyncingObserver {
    feed: Arc<nmp_feed::FlatFeed<nmp_feed::FeedRow>>,
    acquisition_adapter: FeedSessionTrellisAdapter,
    extra_acquisition: crate::source::ExtraAcquisition,
    /// Populated once `DynamicObservedProjectionSet::new` returns — this sink
    /// is constructed BEFORE that call (it IS the observer that call wraps),
    /// so the self-reference is deferred through this slot rather than
    /// requiring a second construction pass.
    engine_observer: Arc<Mutex<Option<crate::dynamic_observer::DynamicObservedProjectionSet>>>,
}

impl ObservedProjectionSink for ResyncingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.feed.on_kernel_event(event);
        let engine_observer = self
            .engine_observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(engine_observer) = engine_observer {
            engine_observer.sync();
        }
        self.acquisition_adapter
            .sync(&self.extra_acquisition, "feed-session-acquisition-demand");
    }
}
