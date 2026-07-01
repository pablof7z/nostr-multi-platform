use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{FeedOpenError, FeedSessionHost};
use nmp_core::substrate::{KernelEvent, ObservedProjectionReconciler};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_SIMPLE_GROUPS;
use nmp_planner::InterestShape;

use super::source::{
    AcquisitionInterest, ExtraAcquisition, LiveShape, LiveShapes, OpSessionIdentity, ReducedSource,
    SessionReactivityHook,
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

fn group_event_shapes(
    groups: &BTreeSet<nmp_nip51::SimpleGroupRef>,
    kinds: &BTreeSet<u32>,
) -> Vec<InterestShape> {
    if groups.is_empty() || kinds.is_empty() {
        return Vec::new();
    }
    let mut by_host: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for group in groups {
        if let Some(group_id) = routable_group_id(group) {
            by_host
                .entry(group_id.host_relay_url)
                .or_default()
                .insert(group_id.local_id);
        }
    }

    by_host
        .into_iter()
        .map(|(host, local_ids)| {
            let mut shape = InterestShape {
                kinds: kinds.clone(),
                relay_pin: Some(host),
                ..InterestShape::default()
            };
            shape.tags.insert("h".to_string(), local_ids);
            shape
        })
        .collect()
}

fn group_event_admitted(
    groups: &BTreeSet<nmp_nip51::SimpleGroupRef>,
    kinds: &BTreeSet<u32>,
    event: &KernelEvent,
) -> bool {
    if groups.is_empty() || !kinds.contains(&event.kind) {
        return false;
    }
    let local_ids: BTreeSet<&str> = event
        .tags
        .iter()
        .filter_map(|tag| {
            (tag.first().map(String::as_str) == Some("h"))
                .then(|| tag.get(1).map(String::as_str))
                .flatten()
        })
        .collect();
    if local_ids.is_empty() {
        return false;
    }
    groups.iter().any(|group| {
        let Some(group_id) = routable_group_id(group) else {
            return false;
        };
        local_ids.contains(group_id.local_id.as_str())
            && (event
                .relay_provenance
                .iter()
                .any(|relay| relay == &group_id.host_relay_url)
                || event
                    .relay_provenance
                    .iter()
                    .any(|relay| relay == "local://publish"))
    })
}

fn routable_group_id(group: &nmp_nip51::SimpleGroupRef) -> Option<nmp_nip29::GroupId> {
    let group_id = nmp_nip29::GroupId::new(group.host_relay_url.clone(), group.local_id.clone());
    group_id.require_routable().ok()?;
    Some(group_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::EventId;

    fn groups() -> BTreeSet<nmp_nip51::SimpleGroupRef> {
        [
            nmp_nip51::SimpleGroupRef::new("room-a", "wss://relay-a"),
            nmp_nip51::SimpleGroupRef::new("room-b", "wss://relay-a"),
            nmp_nip51::SimpleGroupRef::new("room-a", "wss://relay-b"),
        ]
        .into_iter()
        .collect()
    }

    fn event(local_id: &str, relay: &str, kind: u32) -> KernelEvent {
        KernelEvent {
            id: EventId::from("01".repeat(32)),
            author: "aa".repeat(32),
            kind,
            created_at: 10,
            tags: vec![vec!["h".to_string(), local_id.to_string()]],
            content: String::new(),
            relay_provenance: vec![relay.to_string()],
        }
    }

    #[test]
    fn group_event_shapes_group_by_host_relay() {
        let shapes = group_event_shapes(&groups(), &BTreeSet::from([1_u32, 9_u32]));
        assert_eq!(shapes.len(), 2);
        let relay_a = shapes
            .iter()
            .find(|shape| shape.relay_pin.as_deref() == Some("wss://relay-a"))
            .expect("relay-a shape");
        assert_eq!(
            relay_a.tags.get("h").cloned().unwrap_or_default(),
            ["room-a".to_string(), "room-b".to_string()]
                .into_iter()
                .collect()
        );
        let relay_b = shapes
            .iter()
            .find(|shape| shape.relay_pin.as_deref() == Some("wss://relay-b"))
            .expect("relay-b shape");
        assert_eq!(
            relay_b.tags.get("h").cloned().unwrap_or_default(),
            ["room-a".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn admission_requires_matching_host_and_h_tag() {
        let groups = groups();
        let kinds = BTreeSet::from([9_u32]);
        assert!(group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-a", 9)
        ));
        assert!(group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-b", 9)
        ));
        assert!(!group_event_admitted(
            &groups,
            &kinds,
            &event("room-b", "wss://relay-b", 9)
        ));
        assert!(!group_event_admitted(
            &groups,
            &kinds,
            &event("room-a", "wss://relay-a", 1)
        ));
    }
}
