//! Compiler-layer reduced-source product.
//!
//! A [`nmp_feed::FeedScope`] resolver reduces framework/protocol state into this
//! substrate output: admission, typed dependent acquisition, pull shape, reset
//! hooks, and observer teardown ids. The session engine consumes this product;
//! it does not know how a follow set, list, tag, thread, or ranking source was
//! reduced.

use std::sync::Arc;

use nmp_core::{DependentInterestChild, ObservedProjectionId};
use nmp_feed::{FollowPredicate, RootAdmission, TeardownAction};
use nmp_planner::{InterestScope, InterestShape};

/// A closure that, given the feed-window reset trigger, installs it on the
/// underlying set's change signal.
pub(super) type ResetHook = Box<dyn FnOnce(Arc<dyn Fn() + Send + Sync>)>;

/// A closure that installs the session reconciler on a graph-proven source
/// effect.
pub(super) type SourceEffectHook = Box<dyn FnOnce(Arc<dyn Fn() + Send + Sync>)>;

/// The single render/pull acquisition shape, re-read live.
pub(super) type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

/// Extra acquisition shapes a scope must subscribe to beyond the render shape.
pub(super) type ExtraAcquisition = Arc<dyn Fn() -> Vec<AcquisitionInterest> + Send + Sync>;

/// Whether a session can be registered before the active-account slot is
/// populated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OpSessionIdentity {
    RequireActive,
    AllowMissingActive,
}

impl OpSessionIdentity {
    pub(super) fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::AllowMissingActive, Self::AllowMissingActive) => Self::AllowMissingActive,
            _ => Self::RequireActive,
        }
    }
}

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
    /// Session bootstrap policy. Most scopes require an active viewer at open;
    /// `ActiveUserFollows` is intentionally view-driven and may open before
    /// sign-in, with acquisition failing closed until identity resolves.
    pub op_session_identity: OpSessionIdentity,
    /// The engine's event-aware root-admission predicate.
    pub admission: RootAdmission,
    /// The OP-feed attribution predicate.
    ///
    /// This is intentionally separate from root admission. A followed user's
    /// reply can surface a root whose author is outside the source set: the root
    /// enters because it is referenced by an admitted attribution, not because
    /// the root author is part of the source set.
    pub attribution: FollowPredicate,
    /// Fixed typed acquisition interests.
    pub interests: Vec<AcquisitionInterest>,
    /// Live pull acquisition shape.
    pub live_shape: LiveShape,
    /// Extra acquisition that may change as the source projection changes.
    pub extra_acquisition: ExtraAcquisition,
    /// Legacy reactive-reset installers for sources not yet on graph effects.
    pub reset_hooks: Vec<ResetHook>,
    /// Graph source-effect installers. These carry source-set changes through
    /// the same dependent acquisition replacement and feed reset path.
    pub source_effect_hooks: Vec<SourceEffectHook>,
    /// Resolver observer ids the session must revoke on close.
    pub resolver_observer_ids: Vec<ObservedProjectionId>,
    /// Identity-change observer ids the session must revoke on close.
    pub identity_observer_ids: Vec<crate::IdentityChangeObserverId>,
    /// Resolver-owned dynamic observers that need custom teardown.
    pub resolver_teardown: Vec<TeardownAction>,
    /// Active-follow owner retained for active-follows session diagnostics.
    /// Session reactivity is carried by `source_effect_hooks`.
    pub active_follow_set: Option<Arc<nmp_nip02::ActiveFollowSet>>,
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
