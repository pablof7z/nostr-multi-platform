use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::{KernelEvent, ObservedProjectionReconciler};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_SIMPLE_GROUPS;
use nmp_planner::InterestShape;

use super::nip29_group_context::{group_event_admitted, group_event_context, group_event_shapes};
use super::source::{
    AcquisitionInterest, ExtraAcquisition, LiveShape, LiveShapes, OpSessionIdentity, ReducedSource,
    RowContextProvider, SessionReactivityHook,
};
use super::trellis_resources::FeedSessionRouteProvenance;

pub(super) fn resolve_active_simple_groups(
    app: &impl FeedSessionHost,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let initial_viewer = crate::read_active(&app.active_account_handle());
    let projection = Arc::new(nmp_nip51::SimpleGroupListProjection::new(
        app.active_account_handle(),
    ));
    let resolver_shape_slot = app.active_account_handle();
    let resolver_live_shape: LiveShape = Arc::new(move || {
        let viewer = crate::read_active(&resolver_shape_slot)?;
        Some(simple_group_list_shape(&viewer))
    });
    let projection_observer: Arc<dyn ObservedProjectionSink> = projection.clone();
    let resolver_reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        projection_observer,
        "nmp.feed.resolver.simple_groups",
        0,
        64,
        resolver_live_shape,
    );
    resolver_reconciler.sync();
    let resolver_for_identity = resolver_reconciler.clone();
    let resolver_for_teardown = resolver_reconciler.clone();
    let projection_for_identity = Arc::clone(&projection);
    let projection_for_replay = Arc::clone(&projection);
    let replay_slot = app.active_account_handle();
    let replay_pull = app.feed_pull_fn();
    let identity_observer_id = app.register_identity_change_observer(move |_| {
        projection_for_identity.notify_account_changed();
        resolver_for_identity.sync();
        if let Some(viewer) = crate::read_active(&replay_slot) {
            super::source_replay::replay_source_shape_with_pull(
                &replay_pull,
                projection_for_replay.as_ref(),
                simple_group_list_shape(&viewer),
            );
        }
    });
    if let Some(viewer) = initial_viewer {
        super::source_replay::replay_source_shape(
            app,
            projection.as_ref(),
            simple_group_list_shape(&viewer),
        );
    }

    let admission = {
        let projection = Arc::clone(&projection);
        let kinds = kinds.clone();
        Arc::new(move |event: &KernelEvent| {
            group_event_admitted(&projection.groups(), &kinds, event)
        }) as nmp_feed::RootAdmission
    };
    let attribution: nmp_feed::FollowPredicate = Arc::new(|_pubkey: &str| false);
    let live_shapes = group_event_live_shapes(&projection, kinds);
    let live_shape: LiveShape = {
        let live_shapes = Arc::clone(&live_shapes);
        Arc::new(move || live_shapes().into_iter().next())
    };

    let reactivity_hooks = {
        let projection = Arc::clone(&projection);
        vec![Box::new(move |trigger: Arc<dyn Fn() + Send + Sync>| {
            projection.on_source_effect(Box::new(move |_| trigger()));
        }) as SessionReactivityHook]
    };
    let extra_acquisition =
        active_simple_groups_extra_acquisition(app.active_account_handle(), &projection, kinds);
    let row_context: RowContextProvider = {
        let projection = Arc::clone(&projection);
        let kinds = kinds.clone();
        Arc::new(move |event: &KernelEvent| {
            group_event_context(&projection.groups(), &kinds, event)
        })
    };

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::AllowMissingActive,
        admission,
        attribution,
        interests: Vec::new(),
        live_shape,
        live_shapes,
        observer_scope: nmp_planner::InterestScope::Global,
        extra_acquisition,
        reactivity_hooks,
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: vec![identity_observer_id],
        resolver_teardown: vec![Box::new(move || resolver_for_teardown.close_current())],
        active_follow_set: None,
        row_context,
    })
}

fn simple_group_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_SIMPLE_GROUPS].into_iter().collect(),
    )
}

fn active_simple_groups_extra_acquisition(
    slot: nmp_core::slots::ActiveAccountSlot,
    projection: &Arc<nmp_nip51::SimpleGroupListProjection>,
    kinds: &BTreeSet<u32>,
) -> ExtraAcquisition {
    let projection = Arc::clone(projection);
    let kinds = kinds.clone();
    Arc::new(move || {
        let mut shapes = Vec::new();
        if let Some(viewer) = crate::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account_with_provenance(
                simple_group_list_shape(&viewer),
                FeedSessionRouteProvenance::Nip29GroupTimeline,
            ));
        }
        shapes.extend(
            group_event_shapes(&projection.groups(), &kinds)
                .into_iter()
                .map(|shape| {
                    AcquisitionInterest::global_with_provenance(
                        shape,
                        FeedSessionRouteProvenance::Nip29GroupTimeline,
                    )
                }),
        );
        shapes
    })
}

fn group_event_live_shapes(
    projection: &Arc<nmp_nip51::SimpleGroupListProjection>,
    kinds: &BTreeSet<u32>,
) -> LiveShapes {
    let projection = Arc::clone(projection);
    let kinds = kinds.clone();
    Arc::new(move || group_event_shapes(&projection.groups(), &kinds))
}
