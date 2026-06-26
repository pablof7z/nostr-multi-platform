//! Compiler-layer reduced-source product.
//!
//! A [`nmp_feed::FeedScope`] resolver reduces framework/protocol state into this
//! substrate output: admission, typed dependent acquisition, pull shape, reset
//! hooks, and observer teardown ids. The session engine consumes this product;
//! it does not know how a follow set, list, tag, thread, or ranking source was
//! reduced.

use std::sync::Arc;

use nmp_core::{DependentInterestChild, KernelEventObserverId};
use nmp_feed::RootAdmission;
use nmp_planner::{InterestScope, InterestShape};

/// A closure that, given the feed-window reset trigger, installs it on the
/// underlying set's change signal.
pub(super) type ResetHook = Box<dyn FnOnce(Arc<dyn Fn() + Send + Sync>)>;

/// The single render/pull acquisition shape, re-read live.
pub(super) type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

/// Extra acquisition shapes a scope must subscribe to beyond the render shape.
pub(super) type ExtraAcquisition = Arc<dyn Fn() -> Vec<AcquisitionInterest> + Send + Sync>;

/// One typed acquisition child compiled by a reduced source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AcquisitionInterest {
    pub shape: InterestShape,
    pub scope: InterestScope,
}

impl AcquisitionInterest {
    pub(super) fn active_account(shape: InterestShape) -> Self {
        Self {
            shape,
            scope: InterestScope::ActiveAccount,
        }
    }

    pub(super) fn global(shape: InterestShape) -> Self {
        Self {
            shape,
            scope: InterestScope::Global,
        }
    }

    fn into_child(self) -> DependentInterestChild {
        DependentInterestChild::tailing(self.shape, self.scope)
    }
}

/// The compiled product of one reduced feed source.
pub(super) struct ReducedSource {
    /// The engine's event-aware root-admission predicate.
    pub admission: RootAdmission,
    /// Fixed typed acquisition interests.
    pub interests: Vec<AcquisitionInterest>,
    /// Live pull acquisition shape.
    pub live_shape: LiveShape,
    /// Extra acquisition that may change as the source projection changes.
    pub extra_acquisition: ExtraAcquisition,
    /// Reactive-reset installers.
    pub reset_hooks: Vec<ResetHook>,
    /// Resolver observer ids the session must revoke on close.
    pub resolver_observer_ids: Vec<KernelEventObserverId>,
    /// Identity-change observer ids the session must revoke on close.
    pub identity_observer_ids: Vec<nmp_ffi::IdentityChangeObserverId>,
}

/// No extra acquisition beyond fixed interests.
pub(super) fn empty_extra() -> ExtraAcquisition {
    Arc::new(Vec::new)
}

pub(super) fn acquisition_children(
    fixed: &[AcquisitionInterest],
    extra: &ExtraAcquisition,
) -> Vec<DependentInterestChild> {
    fixed
        .iter()
        .cloned()
        .chain(extra())
        .map(AcquisitionInterest::into_child)
        .collect()
}
