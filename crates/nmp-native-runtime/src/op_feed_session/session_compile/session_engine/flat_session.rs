use std::sync::Arc;

use crate::{FeedOpenError, NmpApp};
use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedController, FeedReset, FeedSessionBuild,
    PullFeedController,
};

use super::{
    clear_acquisition_set, observed_projection_scope, session_acquisition_owner,
    visible_flat_payload,
};
use crate::op_feed_session::session_compile::source::{
    acquisition_children, ExtraAcquisition, ReducedSource,
};

pub(super) fn build_flat_scope_session(
    app: &NmpApp,
    key: &str,
    resolved: ReducedSource,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let ReducedSource {
        op_session_identity: _,
        admission,
        attribution: _,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks,
        source_effect_hooks,
        resolver_observer_ids,
        identity_observer_ids,
        resolver_teardown,
        active_follow_set: _,
    } = resolved;

    let feed = nmp_note_feed::FlatFeed::new(admission);
    let observer_for_registry: Arc<dyn ObservedProjectionSink> = feed.clone();
    let engine_observer = crate::op_feed_session::dynamic_observer::DynamicObservedProjection::new(
        app.observed_projection_handle(),
        observer_for_registry,
        format!("{key}.observer"),
        observed_projection_scope(&interests),
        live_shape.clone(),
        512,
    );
    engine_observer.sync();

    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let live_shape = live_shape.clone();
        Arc::new(ClosureInterestShape::new(move || live_shape()))
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
        PullFeedController::new_with_perspective(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(key.to_string(), controller.clone());

    let feed_for_typed = Arc::clone(&feed);
    let typed_key = key.to_string();
    let source = nmp_feed::FeedRenderSource::new(move || feed_for_typed.snapshot_current_window());
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
    let replayed_tail = app.load_older_feed(key);
    let replayed_ids = super::super::flat_replay::replay_fixed_event_ids(app, &feed, &interests);
    if replayed_ids && !replayed_tail {
        (app.feed_teardown().mark_changed())();
    }

    let sender = app.command_sender();
    let owner = session_acquisition_owner(key);
    let fixed_acquisition = Arc::new(interests);
    let sync_acquisition = {
        let sender = sender.clone();
        let fixed_acquisition = Arc::clone(&fixed_acquisition);
        move |extra: &ExtraAcquisition| {
            let children = acquisition_children(&fixed_acquisition, extra);
            let _ = sender.send(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason: "feed-session-acquisition".to_string(),
                },
            ));
        }
    };
    sync_acquisition(&extra_acquisition);
    for hook in reset_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let sync_acquisition = sync_acquisition.clone();
        let sync_observer = engine_observer.clone();
        let notify = sender.clone();
        let reset_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_observer.sync();
            sync_acquisition(&extra);
            let reset = controller_for_reset.reset();
            let replayed = controller_for_reset.load_older();
            if reset || replayed {
                notify.mark_changed_since_emit();
            }
        });
        hook(reset_trigger);
    }
    for hook in source_effect_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let sync_acquisition = sync_acquisition.clone();
        let sync_observer = engine_observer.clone();
        let notify = sender.clone();
        let source_effect_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_observer.sync();
            sync_acquisition(&extra);
            let reset = controller_for_reset.reset();
            let replayed = controller_for_reset.load_older();
            if reset || replayed {
                notify.mark_changed_since_emit();
            }
        });
        hook(source_effect_trigger);
    }

    let teardown_handle = app.feed_teardown();
    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(teardown_handle.mark_changed());
    teardown.push(clear_acquisition_set(sender.clone(), owner));
    teardown.push(teardown_handle.remove_projection(key.to_string()));
    for id in &resolver_observer_ids {
        teardown.push(teardown_handle.revoke_observer(*id));
    }
    for id in &identity_observer_ids {
        teardown.push(teardown_handle.revoke_identity_observer(*id));
    }
    teardown.extend(resolver_teardown);
    teardown.push(engine_observer.teardown_action());
    teardown.push(teardown_handle.unregister_feed(key.to_string()));

    Ok(FeedSessionBuild {
        projection_key: nmp_feed::ProjectionKey::app_owned(key).unwrap(),
        teardown,
    })
}
