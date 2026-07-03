//! The read-lifecycle host seam + the declarative [`ReadSpec`] a concept supplies.
//!
//! A runtime implements [`ReadHost`] ONCE, generically (`NmpApp: ReadHost`); a
//! concept crate (e.g. `nmp-replies`) depends on this engine and calls
//! [`crate::open_read`] with a [`ReadSpec`], never on a runtime crate. The
//! dependency direction is concept → engine ← runtime: the engine names no
//! concept, the concept names no runtime, and a runtime that never imports a
//! concept crate has none of that concept's symbols (owner directive, #2777).
//!
//! The seam is deliberately small and mechanical. It exposes the boring
//! primitives the engine composes — install a typed output, open a
//! replay-before-live observed interest, and record the reverse-teardown steps
//! that withdraw exactly what was wired — plus access to the ONE shared
//! [`crate::ReadSessionRegistry`]. Concepts touch none of these directly; they
//! only describe demand, reducer, and output.

use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, ObservedProjectionSink, TypedProjectionData};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_planner::InterestShape;

use crate::registry::{ReadSessionBuild, ReadSessionId, TeardownAction};

/// The typed-output encoder a concept supplies: a non-blocking closure the host
/// calls on every snapshot tick, returning `Some` when it has a changed row to
/// emit and `None` to retain the last value (coalesced emission is host-owned,
/// ADR-0070 revision ladder / ADR-0072).
pub type ReadOutputEncoder = Box<dyn Fn() -> Option<TypedProjectionData> + Send + Sync>;

/// The declarative dependent-demand provider a concept may supply when the
/// read's future demand depends on events the reducer has admitted.
///
/// The concept computes the current desired demand from its own read-model
/// state (for example, "kind:5 deletes naming every currently admitted wrapper
/// id"). The engine owns the lifecycle: compare, open/replay/live, close the
/// old derived demand, and include the current derived demand in the session's
/// close recipe.
pub type ReadDependentDemandProvider = Arc<dyn Fn() -> Option<ReadDependentDemand> + Send + Sync>;

/// One engine-owned derived observed demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadDependentDemand {
    /// The current shape to observe. Its `relay_pin` is preserved through the
    /// observed-projection open path.
    pub shape: InterestShape,
    /// `0` = `ActiveAccount` (re-routed on account switch), `1` = `Global`.
    pub scope: u32,
    /// Whether sparse author-unknown/account discovery demand should use the
    /// planner's indexer-discovery routing lane.
    pub is_indexer_discovery: bool,
    /// Maximum number of cached events to replay before activation.
    pub replay_limit: usize,
}

/// How an observed demand should seed before live activation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadReplayPolicy {
    /// Use the demand filter as the structural read-cache replay selector.
    Structural,
    /// Open only the live interest. The concept supplied a stronger seed path
    /// (for example NIP-50 FTS search, which must evaluate query text).
    LiveOnly,
}

/// Cloneable hooks the engine can retain after open to reconcile derived
/// observed demands from an event callback. Runtime hosts implement this once;
/// concept crates never see it.
#[derive(Clone)]
pub struct ReadInterestController {
    open: Arc<dyn Fn(ObservedProjection) -> ObservedProjectionId + Send + Sync>,
    close: Arc<dyn Fn(ObservedProjectionId) + Send + Sync>,
}

impl ReadInterestController {
    #[must_use]
    pub fn new(
        open: impl Fn(ObservedProjection) -> ObservedProjectionId + Send + Sync + 'static,
        close: impl Fn(ObservedProjectionId) + Send + Sync + 'static,
    ) -> Self {
        Self {
            open: Arc::new(open),
            close: Arc::new(close),
        }
    }

    #[must_use]
    pub(crate) fn open(&self, decl: ObservedProjection) -> ObservedProjectionId {
        (self.open)(decl)
    }

    pub(crate) fn close(&self, id: ObservedProjectionId) {
        (self.close)(id);
    }
}

/// One routed demand of a read: a compiled NIP-01 `REQ` filter plus its refcount
/// owner + scope + optional relay pin. A read may carry several (e.g. a reply
/// read composes a NIP-10 kind:1 demand and a NIP-22 kind:1111 demand), all
/// feeding the read's single reducer; the engine opens each with
/// replay-before-live and records the exact withdrawal for each.
pub struct ReadDemand {
    /// NIP-01 `REQ` filter JSON selecting this demand's events.
    pub filter_json: String,
    /// Refcount owner key (unique per open screen/component).
    pub consumer_id: String,
    /// `0` = `ActiveAccount` (re-routed on account switch), `1` = `Global`.
    pub scope: u32,
    /// When `Some`, pins the demand to exactly one relay (bypasses outbox
    /// routing). The matching close passes the same pin.
    pub relay_pin: Option<String>,
    /// Whether sparse author-unknown/account discovery demand should use the
    /// planner's indexer-discovery routing lane.
    pub is_indexer_discovery: bool,
    /// Maximum number of cached events to replay before activation.
    pub replay_limit: usize,
    /// Whether this demand uses structural replay or a concept-supplied seed.
    pub replay: ReadReplayPolicy,
}

/// Everything a concept declares to open one read. The engine ([`crate::open_read`])
/// owns the mechanics; a concept owns only these declarative parts.
pub struct ReadSpec {
    /// The framework/app projection key this read's typed output surfaces under.
    pub projection_key: ProjectionRegistrationKey,
    /// The routed demand(s) this read keeps live. Empty only when the concept
    /// supplied a seed-only read and explicitly allows it via
    /// [`Self::keep_open_without_live_demand`].
    pub demands: Vec<ReadDemand>,
    /// The admission-applying event reducer + typed read model. Shared by every
    /// demand of this read (one reducer, one output).
    pub observer: Arc<dyn ObservedProjectionSink>,
    /// The typed-output encoder registered under `projection_key`.
    pub output_encoder: ReadOutputEncoder,
    /// Optional engine-owned dependent demand stages. Empty for fixed reads.
    pub dependent_demands: Vec<ReadDependentDemandProvider>,
    /// Keep the output + close handle live even when no primary live demand
    /// opens. Used by seed-only reads such as cache-only NIP-50 search.
    pub keep_open_without_live_demand: bool,
}

/// The typed close handle a concept read returns. Pairs the opaque projection
/// key the read emits under with the engine-minted session id; the id is the
/// only thing [`crate::close_read`] needs (a concept never re-derives a filter
/// or a raw key to close).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadHandle {
    /// The projection key whose snapshots this read emits (opaque string).
    pub projection_key: String,
    /// The engine-minted session id addressing the live read.
    pub session_id: ReadSessionId,
}

/// The runtime seam the engine drives. A runtime implements this once for its
/// app host; concepts never see the implementation, only [`crate::open_read`].
pub trait ReadHost {
    /// Install the read's typed output under `key` (coalesced emission +
    /// tombstone-on-remove are host-owned). Last-writer-wins per key.
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder);

    /// Open ONE replay-before-live observed interest and return the withdrawal
    /// id. The host replays cached matches into the (muted) observer, then
    /// activates it, in one call, so no matching event is missed. A failed open
    /// returns `ObservedProjectionId(0)`.
    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId;

    /// Open one observed interest without generic structural replay. Concepts
    /// use this only through [`ReadReplayPolicy::LiveOnly`].
    fn open_live_only_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.open_read_interest(decl)
    }

    /// Build the teardown step that withdraws the observed interest `id` (closes
    /// its `REQ` + unregisters the observer). Runs first on close.
    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction;

    /// Build the teardown step that removes (tombstones) the typed output under
    /// `key`.
    fn teardown_remove_output(&self, key: String) -> TeardownAction;

    /// Build the teardown step that flags the next snapshot tick to reflect the
    /// removals. Runs last on close.
    fn teardown_mark_changed(&self) -> TeardownAction;

    /// Record `build` in the ONE shared read-session registry, returning the
    /// minted id (`ReadSessionId(0)` on failure).
    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId;

    /// The projection key of the live read session `id`, for the handle-owns
    /// check before close.
    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String>;

    /// Close the read session `id`, running its reverse teardown once.
    fn close_read_session(&self, id: &ReadSessionId) -> bool;

    /// Close a live read by projection key. This is the generic compatibility
    /// path for legacy hosts that can only name a stable output key; concepts do
    /// not own a separate close map.
    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool;

    /// Optional cloneable controller for engine-owned derived observed demands.
    ///
    /// Hosts that support dependent demand return hooks equivalent to
    /// [`Self::open_read_interest`] and its close teardown, but cloneable so the
    /// engine can reconcile from read-model event callbacks after `open_read`
    /// returns. Concepts never call these hooks directly.
    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        None
    }
}
