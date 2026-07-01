use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::{FeedOpenError, FeedSessionHost};
use nmp_content::{EmbedTarget, PointerSortMode, PointerSourceModel};
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::RootAdmission;
use nmp_nip09::AddressCoordinate;
use nmp_planner::{InterestShape, NaddrCoord};

use super::resolve::{not_supported, resolve_scope};
use super::source::{
    one_live_shape, AcquisitionInterest, ExtraAcquisition, LiveShape, ReducedSource,
    SessionReactivityHook,
};
use super::trellis_resources::FeedSessionRouteProvenance;

type ResetSlot = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

pub(super) fn resolve_pointer_targets(
    app: &impl FeedSessionHost,
    pointers: &nmp_feed::FeedScope,
    pointer_kinds: &[u32],
    primary_kinds: &BTreeSet<u32>,
) -> Result<ReducedSource, FeedOpenError> {
    if pointer_kinds.is_empty() {
        return Err(not_supported("PointerTargets-no-pointer-kinds"));
    }
    if primary_kinds.is_empty() {
        return Err(not_supported("PointerTargets-no-primary-kinds"));
    }

    let pointer_kind_set: BTreeSet<u32> = pointer_kinds.iter().copied().collect();
    let pointer_source = resolve_scope(app, pointers, &pointer_kind_set)?;

    let model = Arc::new(Mutex::new(PointerSourceModel::new(PointerSortMode::Time)));
    let reset_slot: ResetSlot = Arc::new(Mutex::new(None));
    let pointer_observer: Arc<dyn ObservedProjectionSink> = Arc::new(PointerIngest {
        model: Arc::clone(&model),
        pointer_admission: Arc::clone(&pointer_source.admission),
        pointer_kinds: pointer_kind_set,
        reset_slot: Arc::clone(&reset_slot),
    });
    let pointer_dynamic = crate::dynamic_observer::DynamicObservedProjectionSet::new(
        app.observed_projection_handle(),
        pointer_observer,
        "nmp.feed.resolver.pointer_targets.pointer",
        interest_scope_code(pointer_source.observer_scope),
        Arc::clone(&pointer_source.live_shapes),
        512,
    );
    pointer_dynamic.sync();

    let admission = target_admission(&model, primary_kinds);
    let attribution = pointer_source.attribution.clone();
    let live_shape = target_live_shape(&model, primary_kinds);
    let live_shapes = one_live_shape(Arc::clone(&live_shape));

    let mut reactivity_hooks = Vec::new();
    for hook in pointer_source.reactivity_hooks {
        let model = Arc::clone(&model);
        let pointer_dynamic = pointer_dynamic.clone();
        reactivity_hooks.push(Box::new(move |trigger: Arc<dyn Fn() + Send + Sync>| {
            hook(Arc::new(move || {
                lock(&model).clear();
                pointer_dynamic.sync();
                trigger();
            }));
        }) as SessionReactivityHook);
    }
    reactivity_hooks.push(Box::new(move |trigger| {
        *lock(&reset_slot) = Some(trigger);
    }));

    let extra_acquisition =
        pointer_target_extra_acquisition(pointer_source.extra_acquisition, &model, primary_kinds);

    Ok(ReducedSource {
        op_session_identity: pointer_source.op_session_identity,
        admission,
        attribution,
        interests: pointer_source.interests,
        live_shape,
        live_shapes,
        observer_scope: nmp_planner::InterestScope::Global,
        extra_acquisition,
        reactivity_hooks,
        resolver_observer_ids: pointer_source.resolver_observer_ids,
        identity_observer_ids: pointer_source.identity_observer_ids,
        resolver_teardown: {
            let mut teardown = pointer_source.resolver_teardown;
            teardown.push(pointer_dynamic.teardown_action());
            teardown
        },
        active_follow_set: None,
    })
}

fn interest_scope_code(scope: nmp_planner::InterestScope) -> u32 {
    match scope {
        nmp_planner::InterestScope::ActiveAccount => 0,
        nmp_planner::InterestScope::Global => 1,
        nmp_planner::InterestScope::Account(_) => 0,
    }
}

struct PointerIngest {
    model: Arc<Mutex<PointerSourceModel>>,
    pointer_admission: RootAdmission,
    pointer_kinds: BTreeSet<u32>,
    reset_slot: ResetSlot,
}

impl ObservedProjectionSink for PointerIngest {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.pointer_kinds.contains(&event.kind) || !(self.pointer_admission)(event) {
            return;
        }
        if !lock(&self.model).apply_pointer(event) {
            return;
        }
        if let Some(reset) = lock(&self.reset_slot).as_ref().cloned() {
            reset();
        }
    }
}

fn pointer_target_extra_acquisition(
    pointer_extra: ExtraAcquisition,
    model: &Arc<Mutex<PointerSourceModel>>,
    primary_kinds: &BTreeSet<u32>,
) -> ExtraAcquisition {
    let model = Arc::clone(model);
    let primary_kinds = primary_kinds.clone();
    Arc::new(move || {
        let mut interests = pointer_extra();
        interests.extend(
            lock(&model)
                .target_demand()
                .filter_map(|target| target_shape(target, &primary_kinds))
                .map(|shape| {
                    AcquisitionInterest::global_with_provenance(
                        shape,
                        FeedSessionRouteProvenance::PointerTargetHydration,
                    )
                }),
        );
        interests
    })
}

fn target_live_shape(
    model: &Arc<Mutex<PointerSourceModel>>,
    primary_kinds: &BTreeSet<u32>,
) -> LiveShape {
    let model = Arc::clone(model);
    let primary_kinds = primary_kinds.clone();
    Arc::new(move || target_delivery_shape(&lock(&model), &primary_kinds))
}

fn target_admission(
    model: &Arc<Mutex<PointerSourceModel>>,
    primary_kinds: &BTreeSet<u32>,
) -> RootAdmission {
    let model = Arc::clone(model);
    let primary_kinds = primary_kinds.clone();
    Arc::new(move |event: &KernelEvent| {
        primary_kinds.contains(&event.kind) && target_is_demanded(&lock(&model), event)
    })
}

fn target_delivery_shape(
    model: &PointerSourceModel,
    primary_kinds: &BTreeSet<u32>,
) -> Option<InterestShape> {
    let mut shape = InterestShape::default();
    for target in model.target_demand() {
        match target_shape(target, primary_kinds) {
            Some(target_shape) => {
                shape.kinds.extend(target_shape.kinds);
                shape.event_ids.extend(target_shape.event_ids);
                shape.addresses.extend(target_shape.addresses);
            }
            None => continue,
        }
    }
    if shape.event_ids.is_empty() && shape.addresses.is_empty() {
        None
    } else {
        Some(shape)
    }
}

fn target_shape(target: &EmbedTarget, primary_kinds: &BTreeSet<u32>) -> Option<InterestShape> {
    match target {
        EmbedTarget::Event(id) => Some(InterestShape {
            event_ids: BTreeSet::from([id.clone()]),
            kinds: primary_kinds.clone(),
            ..InterestShape::default()
        }),
        EmbedTarget::Address {
            kind,
            pubkey,
            identifier,
        } => primary_kinds.contains(kind).then(|| InterestShape {
            kinds: BTreeSet::from([*kind]),
            addresses: BTreeSet::from([NaddrCoord {
                pubkey: pubkey.clone(),
                kind: *kind,
                d_tag: identifier.clone(),
            }]),
            ..InterestShape::default()
        }),
    }
}

fn target_is_demanded(model: &PointerSourceModel, event: &KernelEvent) -> bool {
    let by_id = EmbedTarget::Event(event.id.clone());
    if model.pointed_by(&by_id).is_empty() {
        if let Some(coord) = AddressCoordinate::from_event(event) {
            let by_addr = EmbedTarget::Address {
                kind: coord.kind,
                pubkey: coord.pubkey,
                identifier: coord.identifier,
            };
            return !model.pointed_by(&by_addr).is_empty();
        }
        false
    } else {
        true
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use nmp_core::substrate::EventId;

    use super::*;

    fn pointer(id: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: EventId::from(id),
            author: "alice".to_string(),
            kind,
            created_at: 100,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    fn target(id: &str, kind: u32, d: Option<&str>) -> KernelEvent {
        let mut tags = Vec::new();
        if let Some(d) = d {
            tags.push(vec!["d".to_string(), d.to_string()]);
        }
        KernelEvent {
            id: EventId::from(id),
            author: "bob".to_string(),
            kind,
            created_at: 110,
            tags,
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn target_delivery_shape_filters_address_targets_to_primary_kind() {
        let mut model = PointerSourceModel::default();
        model.apply_pointer(&pointer(
            "p1",
            7,
            vec![vec!["a", "30023:bob:article"], vec!["a", "30024:bob:draft"]],
        ));

        let shape = target_delivery_shape(&model, &BTreeSet::from([30_023])).expect("shape");
        assert_eq!(shape.kinds, BTreeSet::from([30_023]));
        assert_eq!(
            shape.addresses,
            BTreeSet::from([NaddrCoord {
                pubkey: "bob".to_string(),
                kind: 30_023,
                d_tag: "article".to_string(),
            }])
        );
    }

    #[test]
    fn event_id_target_hydration_is_primary_kind_gated() {
        let mut model = PointerSourceModel::default();
        model.apply_pointer(&pointer("p1", 1111, vec![vec!["E", "root-id"]]));
        let shape = target_delivery_shape(&model, &BTreeSet::from([30_023])).expect("shape");
        assert_eq!(shape.event_ids, BTreeSet::from(["root-id".to_string()]));
        assert_eq!(shape.kinds, BTreeSet::from([30_023]));
    }

    #[test]
    fn admission_accepts_only_demanded_primary_targets() {
        let model = Arc::new(Mutex::new(PointerSourceModel::default()));
        lock(&model).apply_pointer(&pointer(
            "p1",
            7,
            vec![vec!["a", "30023:bob:article"], vec!["e", "event-id"]],
        ));
        let admit = target_admission(&model, &BTreeSet::from([30_023]));

        assert!(admit(&target("event-id", 30_023, None)));
        assert!(!admit(&target("event-id", 1, None)));
        assert!(admit(&target("v1", 30_023, Some("article"))));
        assert!(!admit(&target("other", 30_023, Some("other"))));
    }
}
