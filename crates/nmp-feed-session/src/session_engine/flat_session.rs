use std::sync::Arc;

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

pub(super) fn build_flat_scope_session(
    app: &impl FeedSessionHost,
    key: &str,
    window: FeedWindowPolicy,
    resolved: ReducedSource,
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
        row_context,
    } = resolved;

    // Generic flat engine over the kind-agnostic `FeedRow`. The four knobs:
    //   admission  = the compiled perspective predicate (`admission`);
    //   identity   = NIP-18 repost → target id (in `feed_row_builder`);
    //   sort/merge = `timeline_merge` (repost bump + hydrate).
    let feed = nmp_feed::FlatFeed::with_merge_and_window_policy(
        admission,
        nmp_note_feed::feed_row_builder(row_context),
        None,
        nmp_note_feed::timeline_merge(),
        window,
    );
    let observer_for_registry: Arc<dyn ObservedProjectionSink> = feed.clone();
    let engine_observer = crate::dynamic_observer::DynamicObservedProjectionSet::new(
        app.observed_projection_handle(),
        observer_for_registry,
        format!("{key}.observer"),
        interest_scope_code(observer_scope),
        live_shapes.clone(),
        512,
    );
    engine_observer.sync();

    let provider: Arc<dyn nmp_feed::FeedInterestShapes + Send + Sync> = {
        let live_shapes = live_shapes.clone();
        Arc::new(ClosureInterestShapes::new(move || live_shapes()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let feed = Arc::clone(&feed);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_flat_payload(&feed);
            feed.on_kernel_event(event);
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
        // PROVISIONAL feed-row wire (serde-JSON, `NFRW`) — TODO(#3082): replace
        // with the frozen typed FlatBuffers wire once the FeedRow shape settles.
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_note_feed::FEED_ROW_SCHEMA_ID.to_string(),
            schema_version: nmp_note_feed::FEED_ROW_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_note_feed::FEED_ROW_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_note_feed::encode_feed_row_snapshot(snapshot),
            ..Default::default()
        })
    });
    let sender = app.command_sender();
    let acquisition_adapter = FeedSessionTrellisAdapter::new_with_diagnostics(
        key,
        FeedShape::Flat,
        interests.clone(),
        sender,
        app.feed_session_diagnostics(),
    )?;
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
