//! Composition root for the pointer-source target-hydration read model (#2113).
//!
//! This wires [`nmp_content::PointerSourceModel`] (pure read-model state) to the
//! kernel's existing seams so a pointer source behaves like NDK `$metaSubscribe`
//! while keeping every target request on the planner/router/cache path:
//!
//! 1. **Pointer ingest** — the pointer source is one ordinary observed
//!    projection (`ObservedProjection::from_shape`). Its sink feeds each pointer
//!    event into the model.
//! 2. **Target acquisition** — whenever the model's demanded target set changes,
//!    the controller replaces a kernel-owned **dependent-interest set**
//!    (`InterestsCommand::ReplaceDependentInterestSet`), one child per target
//!    (`event_ids` for an [`EmbedTarget::Event`], `addresses` for an
//!    [`EmbedTarget::Address`]). This is the same lifecycle the ReducedSource
//!    feed primitive (#2092) uses: the kernel withdraws disappeared children,
//!    dedups identical children across consumers onto one registry slot, and
//!    closes a slot when its last owner leaves.
//! 3. **Target delivery** — dependent interests *acquire* but do not deliver to a
//!    read model, so a reconciled [`DynamicTargetProjection`] (the same
//!    open/close-on-shape-change pattern the feed's `DynamicObservedProjection`
//!    uses) carries the materialized target events back into the model. This is
//!    the one substrate gap the issue flagged; it is bridged with an existing
//!    seam, not a new trait family.
//!
//! Sort is read-model state: [`PointerSourceSession::set_sort`] reorders output
//! without touching any interest. An empty pointer reduction yields no children
//! and no delivery shape, so demand fails closed (never a wildcard query).

mod shapes;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use nmp_content::{PointerSortMode, PointerSourceModel};
use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionRegistrar,
};
use nmp_core::{CommandSender, ObservedProjectionId, ObservedProjectionSink};
use nmp_planner::InterestShape;

pub(crate) use shapes::{delivery_shape, target_children};

/// Declaration for one pointer-source read model.
pub struct PointerSourceParams {
    /// The pointer interest — which events carry the `e` / `a` references (e.g.
    /// `{kinds: [6, 16], authors: [...]}`). This is a product decision owned by
    /// the caller; the kernel never invents it.
    pub pointer_shape: InterestShape,
    /// Refcount owner key, unique per open read model.
    pub consumer_id: String,
    /// `0` = `ActiveAccount` (re-routes on account switch), any other value =
    /// `Global`.
    pub scope: u32,
    /// Initial projection sort mode.
    pub sort: PointerSortMode,
    /// Cached events replayed before each projection activates.
    pub replay_limit: usize,
    /// Optional change notifier, invoked after the projection changes (pointer
    /// demand shifted or a target hydrated). Callback-driven, never polled (D8).
    pub on_update: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A live pointer-source read model. Drop-free: call [`Self::close`] to release
/// the pointer interest, the dependent target set, and the delivery projection.
pub struct PointerSourceSession {
    model: Arc<Mutex<PointerSourceModel>>,
    on_update: Option<Arc<dyn Fn() + Send + Sync>>,
    teardown: Option<Box<dyn FnOnce() + Send>>,
}

impl PointerSourceSession {
    /// Shared handle to the read-model state for on-demand reads (sorted items,
    /// `pointedBy`). Reads are pull-based; no polling loop is implied.
    #[must_use]
    pub fn model(&self) -> Arc<Mutex<PointerSourceModel>> {
        Arc::clone(&self.model)
    }

    /// Change the projection sort mode. Pure read-model state change: it never
    /// reopens the pointer or target interests.
    pub fn set_sort(&self, sort: PointerSortMode) {
        let changed = lock(&self.model).set_sort(sort);
        if changed {
            if let Some(cb) = &self.on_update {
                cb();
            }
        }
    }

    /// Release the pointer interest, dependent target set, and delivery
    /// projection.
    pub fn close(mut self) {
        if let Some(teardown) = self.teardown.take() {
            teardown();
        }
    }
}

impl Drop for PointerSourceSession {
    fn drop(&mut self) {
        if let Some(teardown) = self.teardown.take() {
            teardown();
        }
    }
}

/// Open a pointer-source read model against explicit kernel handles.
///
/// Most callers use [`register_pointer_source`]; this lower-level form takes the
/// command sender and observed-projection registrar directly so it can be wired
/// from contexts that do not hold a full app handle.
#[must_use]
fn open_pointer_source_internal(
    sender: CommandSender,
    registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
    params: PointerSourceParams,
) -> PointerSourceSession {
    let model = Arc::new(Mutex::new(PointerSourceModel::new(params.sort)));
    let owner = SubOwnerKey::new(&params.consumer_id);

    let target_observer: Arc<dyn ObservedProjectionSink> = Arc::new(TargetIngest {
        model: Arc::clone(&model),
        on_update: params.on_update.clone(),
    });
    let delivery = DynamicTargetProjection {
        registrar: Arc::clone(&registrar),
        observer: target_observer,
        model: Arc::clone(&model),
        consumer_id: format!("{}.target", params.consumer_id),
        scope: params.scope,
        replay_limit: params.replay_limit,
        current: Arc::new(Mutex::new(None)),
    };

    let pointer_observer: Arc<dyn ObservedProjectionSink> = Arc::new(PointerIngest {
        model: Arc::clone(&model),
        sender: sender.clone(),
        owner,
        scope: params.scope,
        delivery: delivery.clone(),
        on_update: params.on_update.clone(),
    });
    let pointer_id = registrar.open_observed_projection(ObservedProjection::from_shape(
        pointer_observer,
        format!("{}.pointer", params.consumer_id),
        params.scope,
        params.pointer_shape,
        params.replay_limit,
    ));

    let teardown_registrar = Arc::clone(&registrar);
    let teardown_delivery = delivery;
    let teardown_sender = sender;
    let teardown: Box<dyn FnOnce() + Send> = Box::new(move || {
        teardown_registrar.close_observed_projection(pointer_id);
        teardown_delivery.close();
        let _ = teardown_sender.send(ActorCommand::Interests(
            InterestsCommand::ReplaceDependentInterestSet {
                owner,
                children: Vec::new(),
                reason: "pointer-source-close".to_string(),
            },
        ));
    });

    PointerSourceSession {
        model,
        on_update: params.on_update,
        teardown: Some(teardown),
    }
}

/// Pointer-event sink: ingest each pointer, and when the demanded target set
/// changes, replace the dependent acquisition set and re-sync delivery.
struct PointerIngest {
    model: Arc<Mutex<PointerSourceModel>>,
    sender: CommandSender,
    owner: SubOwnerKey,
    scope: u32,
    delivery: DynamicTargetProjection,
    on_update: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ObservedProjectionSink for PointerIngest {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let demand_changed = lock(&self.model).apply_pointer(event);
        if !demand_changed {
            return;
        }
        let children = target_children(&lock(&self.model), self.scope);
        let _ = self.sender.send(ActorCommand::Interests(
            InterestsCommand::ReplaceDependentInterestSet {
                owner: self.owner,
                children,
                reason: "pointer-source-acquisition".to_string(),
            },
        ));
        self.delivery.sync();
        if let Some(cb) = &self.on_update {
            cb();
        }
    }
}

/// Target-event sink: hydrate matching targets and notify on projection change.
struct TargetIngest {
    model: Arc<Mutex<PointerSourceModel>>,
    on_update: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ObservedProjectionSink for TargetIngest {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let changed = lock(&self.model).apply_target(event);
        if changed {
            if let Some(cb) = &self.on_update {
                cb();
            }
        }
    }
}

/// One observed projection reconciled to the model's union delivery shape.
///
/// Mirrors the feed's `DynamicObservedProjection`: it opens an observed
/// projection over the current target union (`event_ids` ∪ `addresses`), closing
/// and reopening only when that union shape changes. Sort changes never touch it.
#[derive(Clone)]
struct DynamicTargetProjection {
    registrar: Arc<dyn ObservedProjectionRegistrar + Send + Sync>,
    observer: Arc<dyn ObservedProjectionSink>,
    model: Arc<Mutex<PointerSourceModel>>,
    consumer_id: String,
    scope: u32,
    replay_limit: usize,
    current: Arc<Mutex<Option<(InterestShape, ObservedProjectionId)>>>,
}

impl DynamicTargetProjection {
    fn sync(&self) {
        let desired = delivery_shape(&lock(&self.model));
        let mut current = lock(&self.current);
        let unchanged = current
            .as_ref()
            .map(|(shape, _)| Some(shape) == desired.as_ref())
            .unwrap_or(desired.is_none());
        if unchanged {
            return;
        }
        if let Some((_, id)) = current.take() {
            self.registrar.close_observed_projection(id);
        }
        let Some(shape) = desired else {
            return;
        };
        let id = self
            .registrar
            .open_observed_projection(ObservedProjection::from_shape(
                Arc::clone(&self.observer),
                self.consumer_id.clone(),
                self.scope,
                shape.clone(),
                self.replay_limit,
            ));
        if id.0 != 0 {
            *current = Some((shape, id));
        }
    }

    fn close(&self) {
        if let Some((_, id)) = lock(&self.current).take() {
            self.registrar.close_observed_projection(id);
        }
    }
}

/// Lock a mutex, recovering the guard on poison (sinks must never panic on the
/// actor thread).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
