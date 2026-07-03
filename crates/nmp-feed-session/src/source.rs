//! Compiler-layer reduced-source product.
//!
//! A [`nmp_feed::FeedScope`] resolver reduces framework/protocol state into this
//! substrate output: admission, typed dependent acquisition, pull shape, reset
//! hooks, and observer teardown ids. The session engine consumes this product;
//! it does not know how a follow set, list, tag, thread, or order source was
//! reduced.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionId;
use nmp_feed::{FollowPredicate, RootAdmission, TeardownAction};
use nmp_planner::{InterestScope, InterestShape};

use crate::trellis_resources::{
    FeedSessionResourceKey, FeedSessionRouteProvenance, InterestDemand,
};

/// A closure that installs the session reconciler on a source-change signal.
///
/// Resolvers may obtain that signal from a Trellis-backed source graph, a
/// protocol projection, or a session-local observer. The session engine owns a
/// single response path: resync observed delivery, resync Trellis acquisition,
/// and rebaseline output if the visible window changed.
pub(super) type SessionReactivityHook = Box<dyn FnOnce(Arc<dyn Fn() + Send + Sync>)>;

/// The single render/pull acquisition shape, re-read live.
pub(super) type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;

/// The render/pull acquisition shapes, re-read live.
///
/// Most sources produce one shape. Relay-pinned grouped sources can produce
/// several shapes because different `relay_pin` values must not be merged.
pub(super) type LiveShapes = Arc<dyn Fn() -> Vec<InterestShape> + Send + Sync>;

/// Extra acquisition shapes a scope must subscribe to beyond the render shape.
pub(super) type ExtraAcquisition = Arc<dyn Fn() -> Vec<AcquisitionInterest> + Send + Sync>;

/// Optional source-owned row context attached to emitted note-feed rows.
pub(super) type RowContextProvider =
    Arc<dyn Fn(&KernelEvent) -> Option<nmp_note_feed::HostedGroupContext> + Send + Sync>;

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
    pub provenance: FeedSessionRouteProvenance,
}

impl AcquisitionInterest {
    pub(super) fn active_account_with_provenance(
        shape: InterestShape,
        provenance: FeedSessionRouteProvenance,
    ) -> Self {
        Self {
            shape,
            scope: InterestScope::ActiveAccount,
            provenance,
        }
    }

    pub(super) fn global(shape: InterestShape) -> Self {
        Self::global_with_provenance(shape, FeedSessionRouteProvenance::StaticFeedScope)
    }

    pub(super) fn global_with_provenance(
        shape: InterestShape,
        provenance: FeedSessionRouteProvenance,
    ) -> Self {
        Self {
            shape,
            scope: InterestScope::Global,
            provenance,
        }
    }

    pub(super) fn demand(&self) -> InterestDemand {
        InterestDemand::tailing(&self.scope, self.shape.clone(), self.provenance)
    }

    pub(super) fn resource_key(&self) -> FeedSessionResourceKey {
        self.demand().resource_key()
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
    /// Live row-source shapes used by observed delivery and pull pagination.
    pub live_shapes: LiveShapes,
    /// Scope for the session's observed row-source subscriptions.
    pub observer_scope: InterestScope,
    /// Extra acquisition that may change as the source projection changes.
    pub extra_acquisition: ExtraAcquisition,
    /// Source-change installers. These carry source-set changes through the
    /// Trellis dependent-acquisition delta and feed reset/rebaseline path.
    pub reactivity_hooks: Vec<SessionReactivityHook>,
    /// Resolver observer ids the session must revoke on close.
    pub resolver_observer_ids: Vec<ObservedProjectionId>,
    /// Identity-change observer ids the session must revoke on close.
    pub identity_observer_ids: Vec<crate::IdentityChangeObserverId>,
    /// Resolver-owned dynamic observers that need custom teardown.
    pub resolver_teardown: Vec<TeardownAction>,
    /// Active-follow owner retained for active-follows session diagnostics.
    /// Session reactivity is carried by `reactivity_hooks`.
    pub active_follow_set: Option<Arc<nmp_nip02::ActiveFollowSet>>,
    /// Source-owned context for the event that becomes a feed row.
    pub row_context: RowContextProvider,
}

/// No extra acquisition beyond fixed interests.
pub(super) fn empty_extra() -> ExtraAcquisition {
    Arc::new(Vec::new)
}

pub(super) fn one_live_shape(live_shape: LiveShape) -> LiveShapes {
    Arc::new(move || live_shape().into_iter().collect())
}

pub(super) fn empty_row_context() -> RowContextProvider {
    Arc::new(|_| None)
}
