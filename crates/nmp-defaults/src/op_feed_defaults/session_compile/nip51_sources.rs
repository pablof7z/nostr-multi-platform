use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionSink;
use nmp_feed::RootAdmission;
use nmp_ffi::{FeedOpenError, NmpApp};
use nmp_kinds::KIND_MUTE_LIST;
use nmp_planner::InterestShape;

use super::source::{
    AcquisitionInterest, ExtraAcquisition, LiveShape, OpSessionIdentity, ReducedSource, ResetHook,
};

pub(super) fn resolve_active_mute_list_members(
    app: &NmpApp,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    let viewer = super::super::read_active(&app.active_account_handle()).ok_or_else(|| {
        super::resolve::not_supported("ListMembers-active-mute-no-active-account")
    })?;

    let projection = Arc::new(nmp_nip51::MuteListProjection::new(
        app.active_account_handle(),
    ));
    let observer_id = app.open_observed_projection(ObservedProjection::from_shape(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        "nmp.feed.resolver.active_mute_list",
        0,
        active_mute_list_shape(&viewer),
        64,
    ));
    let projection_for_identity = Arc::clone(&projection);
    let projection_for_replay = Arc::clone(&projection);
    let replay_slot = app.active_account_handle();
    let replay_pull = app.feed_pull_fn();
    let identity_observer_id = app.register_identity_change_observer(move |_| {
        projection_for_identity.notify_account_changed();
        if let Some(viewer) = super::super::read_active(&replay_slot) {
            super::source_replay::replay_source_shape_with_pull(
                &replay_pull,
                projection_for_replay.as_ref(),
                active_mute_list_shape(&viewer),
            );
        }
    });
    super::source_replay::replay_source_shape(
        app,
        projection.as_ref(),
        active_mute_list_shape(&viewer),
    );

    let admission: RootAdmission = {
        let projection = Arc::clone(&projection);
        Arc::new(move |event: &KernelEvent| projection.muted_pubkeys().contains(&event.author))
    };

    let live_shape: LiveShape = {
        let projection = Arc::clone(&projection);
        let kinds = kinds.clone();
        Arc::new(move || {
            let members = projection.muted_pubkeys();
            if members.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                members.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };

    let reset_proj = Arc::clone(&projection);
    let reset_hook: ResetHook = Box::new(move |reset| {
        reset_proj.on_change(Box::new(move || reset()));
    });
    let extra_acquisition = active_mute_list_extra_acquisition(
        app.active_account_handle(),
        &projection,
        kinds,
        &live_shape,
    );

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        interests: Vec::new(),
        extra_acquisition,
        live_shape,
        reset_hooks: vec![reset_hook],
        resolver_observer_ids: vec![observer_id],
        identity_observer_ids: vec![identity_observer_id],
    })
}

fn active_mute_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_MUTE_LIST].into_iter().collect(),
    )
}

fn active_mute_list_extra_acquisition(
    slot: nmp_core::slots::ActiveAccountSlot,
    projection: &Arc<nmp_nip51::MuteListProjection>,
    kinds: &BTreeSet<u32>,
    live_shape: &LiveShape,
) -> ExtraAcquisition {
    let projection = Arc::clone(projection);
    let kinds = kinds.clone();
    let live_shape = Arc::clone(live_shape);
    Arc::new(move || {
        let mut shapes = Vec::new();
        if let Some(viewer) = super::super::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account(active_mute_list_shape(
                &viewer,
            )));
        }
        if !projection.muted_pubkeys().is_empty() && !kinds.is_empty() {
            if let Some(shape) = live_shape() {
                shapes.push(AcquisitionInterest::active_account(shape));
            }
        }
        shapes
    })
}
