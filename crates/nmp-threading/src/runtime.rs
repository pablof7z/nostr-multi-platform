//! Host wiring for the `nmp.threading.graph.*` read model family.
//!
//! #3096 — this used to drive `open_observed_projection` /
//! `register_typed_snapshot_projection` directly: a second, un-audited
//! open/close read lifecycle that bypassed the shared
//! [`nmp_read_session::ReadSessionRegistry`], its reverse-teardown recording,
//! and the `live_count` leak audit every other concept-owned active read
//! (replies/reactions/reposts/feed/zaps/nip29/nip50) goes through. This now
//! composes a caller-supplied [`InterestShape`] scope into a
//! [`nmp_read_session::ReadSpec`] and drives it through the ONE engine
//! (`nmp_read_session::open_read` / `close_read`), exactly like every other
//! concept-owned door (e.g. `nmp_reposts::open_reposts`). It contains NO
//! registry, NO close map, NO replay implementation, and NO teardown recipe
//! of its own.

use std::sync::Arc;

use nmp_core::subs::filter_json_for;
use nmp_core::ObservedProjectionSink;
use nmp_ownership::FrameworkProjectionKey;
use nmp_planner::InterestShape;
use nmp_read_session::{
    close_read, open_read, InterestLifecycle, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder,
    ReadReplayPolicy, ReadSpec,
};

use crate::{ModulePolicy, ThreadingProjection};

/// Projection-family owner claim used by every dynamic threading graph key.
pub const THREADING_GRAPH_PROJECTION_FAMILY_CLAIM: &str = "projection.nmp.threading.graph";
/// Dynamic projection key prefix. Session keys append a validated suffix.
pub const THREADING_GRAPH_PROJECTION_KEY_PREFIX: &str = "nmp.threading.graph.";
/// Stable schema id carried in typed-projection envelopes.
pub const THREADING_GRAPH_SCHEMA_ID: &str = "nmp.threading.graph";
/// Maximum accepted byte length for a caller-supplied session suffix.
pub const THREADING_GRAPH_SESSION_ID_MAX_LEN: usize = 128;

/// Account routing scope for the observed projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadingScope {
    ActiveAccount,
    Global,
}

impl ThreadingScope {
    const fn code(self) -> u32 {
        match self {
            Self::ActiveAccount => 0,
            Self::Global => 1,
        }
    }
}

/// Parameters for one open threading graph read model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadingReadModelParams {
    /// Caller-stable id used to derive `nmp.threading.graph.<session_id>`.
    pub session_id: String,
    /// Event scope to observe and replay.
    pub shape: InterestShape,
    /// Routing scope for the observed projection.
    pub scope: ThreadingScope,
    /// Maximum cached events replayed before live activation.
    pub replay_limit: usize,
    /// Grouping policy for the emitted block layout.
    pub policy: ModulePolicy,
}

impl ThreadingReadModelParams {
    #[must_use]
    pub fn global(session_id: impl Into<String>, shape: InterestShape) -> Self {
        Self {
            session_id: session_id.into(),
            shape,
            scope: ThreadingScope::Global,
            replay_limit: 512,
            policy: ModulePolicy::default(),
        }
    }
}

/// Handle returned by [`open_threading_read_model`]. Wraps the engine's
/// opaque [`ReadHandle`] so a threading read can only be closed with
/// [`close_threading_read_model`] (not with a feed/reply/reaction handle).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadingReadModelHandle(ReadHandle);

impl ThreadingReadModelHandle {
    /// The projection key this read's typed [`crate::ThreadingSnapshot`]
    /// surfaces under. The caller learns it from the handle and renders that
    /// key.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Build the framework-owned projection key for `session_id`.
///
/// The suffix is intentionally conservative so a session id cannot smuggle a
/// second namespace segment with whitespace or control characters, and so the
/// snapshot registry cannot be fed unbounded caller-owned keys.
pub fn threading_projection_key(session_id: &str) -> Option<String> {
    let suffix = session_id.trim();
    if suffix.is_empty()
        || suffix.len() > THREADING_GRAPH_SESSION_ID_MAX_LEN
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(format!("{THREADING_GRAPH_PROJECTION_KEY_PREFIX}{suffix}"))
}

/// Open a reactive threading graph projection against a caller-supplied scope.
///
/// Composes `params.shape` into a single routed demand folding into the
/// [`ThreadingProjection`] reducer, then drives it through the read-lifecycle
/// engine (`nmp-read-session`), which owns replay-before-live ordering, live
/// activation, exact-demand withdrawal, reverse teardown, and typed-output
/// tombstone — landing this read in the ONE shared
/// [`nmp_read_session::ReadSessionRegistry`] leak audit.
pub fn open_threading_read_model(
    host: &dyn ReadHost,
    params: ThreadingReadModelParams,
) -> Option<ThreadingReadModelHandle> {
    let projection_key = threading_projection_key(&params.session_id)?;
    let registration_key =
        FrameworkProjectionKey::declared(projection_key.clone(), "projection.nmp.threading.graph")
            .ok()?;

    let relay_pin = params.shape.relay_pin.clone();
    let filter_json = filter_json_for(&params.shape);

    let demand = ReadDemand {
        filter_json,
        consumer_id: projection_key.clone(),
        scope: params.scope.code(),
        relay_pin,
        is_indexer_discovery: false,
        lifecycle: InterestLifecycle::Tailing,
        replay_limit: params.replay_limit,
        replay: ReadReplayPolicy::Structural,
    };

    let projection = Arc::new(ThreadingProjection::etag(params.policy));

    // Typed output: encode the reducer's snapshot each tick. Coalesced
    // emission + tombstone-on-close are the engine/host's, not this
    // closure's.
    let projection_for_output = Arc::clone(&projection);
    let output_key = projection_key.clone();
    let output_encoder: ReadOutputEncoder =
        Box::new(move || Some(projection_for_output.typed_projection(&output_key)));

    let handle = open_read(
        host,
        ReadSpec {
            projection_key: registration_key.into(),
            demands: vec![demand],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: false,
        },
    );

    Some(ThreadingReadModelHandle(handle))
}

/// Close a threading read model opened by [`open_threading_read_model`].
/// Idempotent (D6): closing an already-closed or stale handle is a safe
/// no-op.
pub fn close_threading_read_model(host: &dyn ReadHost, handle: ThreadingReadModelHandle) -> bool {
    close_read(host, &handle.0)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
