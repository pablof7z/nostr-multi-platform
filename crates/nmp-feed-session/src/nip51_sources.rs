use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::{KernelEvent, ObservedProjectionReconciler};
use nmp_core::ObservedProjectionSink;
use nmp_feed::RootAdmission;
use nmp_kinds::{KIND_FOLLOW_SET, KIND_MUTE_LIST};
use nmp_planner::InterestShape;

use super::resolve::unique_consumer_id;
use super::source::{
    empty_row_context, one_live_shape, AcquisitionInterest, ExtraAcquisition, LiveShape,
    OpSessionIdentity, ReducedSource, SessionReactivityHook,
};
use super::trellis_resources::FeedSessionRouteProvenance;

pub(super) fn resolve_list_members(
    app: &impl FeedSessionHost,
    list_id: &str,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    if list_id == nmp_nip51::ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID {
        return resolve_active_mute_list_members(app, kinds);
    }

    let viewer = crate::read_active(&app.active_account_handle())
        .ok_or_else(|| super::resolve::not_supported("ListMembers-no-active-account"))?;

    let projection = Arc::new(nmp_nip51::PeopleListProjection::new(
        app.active_account_handle(),
    ));
    let resolver_shape_slot = app.active_account_handle();
    let resolver_live_shape: LiveShape = Arc::new(move || {
        let viewer = crate::read_active(&resolver_shape_slot)?;
        Some(viewer_list_shape(&viewer))
    });
    let projection_observer: Arc<dyn ObservedProjectionSink> = projection.clone();
    let resolver_reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        projection_observer,
        unique_consumer_id("nmp.feed.resolver.people_list"),
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
                viewer_list_shape(&viewer),
            );
        }
    });
    super::source_replay::replay_source_shape(app, projection.as_ref(), viewer_list_shape(&viewer));

    let admission: RootAdmission = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        Arc::new(move |event: &KernelEvent| projection.members(&list_id).contains(&event.author))
    };
    let attribution: nmp_feed::FollowPredicate = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        Arc::new(move |pubkey: &str| projection.members(&list_id).contains(pubkey))
    };

    let live_shape: LiveShape = {
        let projection = Arc::clone(&projection);
        let list_id = list_id.to_string();
        let kinds = kinds.clone();
        Arc::new(move || {
            let members = projection.members(&list_id);
            if members.is_empty() || kinds.is_empty() {
                return None;
            }
            Some(InterestShape::timeline_for(
                members.into_iter().collect(),
                kinds.clone(),
            ))
        })
    };
    let live_shapes = one_live_shape(Arc::clone(&live_shape));

    let reactivity_hooks = {
        let projection = Arc::clone(&projection);
        vec![Box::new(move |trigger: Arc<dyn Fn() + Send + Sync>| {
            projection.on_source_effect(Box::new(move |_| trigger()));
        }) as SessionReactivityHook]
    };
    let extra_acquisition = list_members_extra_acquisition(
        app.active_account_handle(),
        &projection,
        list_id,
        kinds,
        &live_shape,
    );

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::RequireActive,
        admission,
        attribution,
        interests: Vec::new(),
        extra_acquisition,
        live_shape,
        live_shapes,
        observer_scope: nmp_planner::InterestScope::ActiveAccount,
        reactivity_hooks,
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: vec![identity_observer_id],
        resolver_teardown: vec![Box::new(move || resolver_for_teardown.close_current())],
        active_follow_set: None,
        row_context: empty_row_context(),
    })
}

pub(super) fn resolve_active_mute_list_members(
    app: &impl FeedSessionHost,
    kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    // No active account yet: degrade gracefully to an empty mute set instead
    // of failing the whole open (mirrors `ActiveUserFollows`'s
    // `AllowMissingActive` semantics — see #2930). This lets
    // `difference(active_user().follows(), list_members(mute))` open
    // pre-login and simply admit everything until sign-in populates both
    // sides.
    let initial_viewer = crate::read_active(&app.active_account_handle());

    let projection = Arc::new(nmp_nip51::MuteListProjection::new(
        app.active_account_handle(),
    ));
    let resolver_shape_slot = app.active_account_handle();
    let resolver_live_shape: LiveShape = Arc::new(move || {
        let viewer = crate::read_active(&resolver_shape_slot)?;
        Some(active_mute_list_shape(&viewer))
    });
    let projection_observer: Arc<dyn ObservedProjectionSink> = projection.clone();
    let resolver_reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        projection_observer,
        unique_consumer_id("nmp.feed.resolver.active_mute_list"),
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
                active_mute_list_shape(&viewer),
            );
        }
    });
    if let Some(viewer) = &initial_viewer {
        super::source_replay::replay_source_shape(
            app,
            projection.as_ref(),
            active_mute_list_shape(viewer),
        );
    }

    let admission: RootAdmission = {
        let projection = Arc::clone(&projection);
        Arc::new(move |event: &KernelEvent| projection.muted_pubkeys().contains(&event.author))
    };
    let attribution: nmp_feed::FollowPredicate = {
        let projection = Arc::clone(&projection);
        Arc::new(move |pubkey: &str| projection.muted_pubkeys().contains(pubkey))
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
    let live_shapes = one_live_shape(Arc::clone(&live_shape));

    let reset_proj = Arc::clone(&projection);
    let reactivity_hook: SessionReactivityHook = Box::new(move |trigger| {
        reset_proj.on_change(Box::new(move || trigger()));
    });
    let extra_acquisition = active_mute_list_extra_acquisition(
        app.active_account_handle(),
        &projection,
        kinds,
        &live_shape,
    );

    Ok(ReducedSource {
        op_session_identity: OpSessionIdentity::AllowMissingActive,
        admission,
        attribution,
        interests: Vec::new(),
        extra_acquisition,
        live_shape,
        live_shapes,
        observer_scope: nmp_planner::InterestScope::ActiveAccount,
        reactivity_hooks: vec![reactivity_hook],
        resolver_observer_ids: Vec::new(),
        identity_observer_ids: vec![identity_observer_id],
        resolver_teardown: vec![Box::new(move || resolver_for_teardown.close_current())],
        active_follow_set: None,
        row_context: empty_row_context(),
    })
}

fn viewer_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_FOLLOW_SET].into_iter().collect(),
    )
}

fn active_mute_list_shape(viewer: &str) -> InterestShape {
    InterestShape::timeline_for(
        [viewer.to_string()].into_iter().collect(),
        [KIND_MUTE_LIST].into_iter().collect(),
    )
}

fn list_members_extra_acquisition(
    slot: nmp_core::slots::ActiveAccountSlot,
    projection: &Arc<nmp_nip51::PeopleListProjection>,
    list_id: &str,
    kinds: &BTreeSet<u32>,
    live_shape: &LiveShape,
) -> ExtraAcquisition {
    let projection = Arc::clone(projection);
    let list_id = list_id.to_string();
    let kinds = kinds.clone();
    let live_shape = Arc::clone(live_shape);
    Arc::new(move || {
        let mut shapes = Vec::new();
        if let Some(viewer) = crate::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account_with_provenance(
                viewer_list_shape(&viewer),
                FeedSessionRouteProvenance::Nip51ListMembers,
            ));
        }
        if !projection.members(&list_id).is_empty() && !kinds.is_empty() {
            if let Some(shape) = live_shape() {
                shapes.push(AcquisitionInterest::active_account_with_provenance(
                    shape,
                    FeedSessionRouteProvenance::Nip51ListMembers,
                ));
            }
        }
        shapes
    })
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
        if let Some(viewer) = crate::read_active(&slot) {
            shapes.push(AcquisitionInterest::active_account_with_provenance(
                active_mute_list_shape(&viewer),
                FeedSessionRouteProvenance::Nip51ListMembers,
            ));
        }
        if !projection.muted_pubkeys().is_empty() && !kinds.is_empty() {
            if let Some(shape) = live_shape() {
                shapes.push(AcquisitionInterest::active_account_with_provenance(
                    shape,
                    FeedSessionRouteProvenance::Nip51ListMembers,
                ));
            }
        }
        shapes
    })
}
