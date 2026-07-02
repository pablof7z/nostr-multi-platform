//! Host wiring for the `nmp.threading.graph.*` read model family.

use std::sync::Arc;

use nmp_core::substrate::{
    ObservedProjection, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::FrameworkProjectionKey;
use nmp_planner::InterestShape;

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

/// Handle returned by [`open_threading_read_model`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadingReadModelHandle {
    projection_key: String,
    observer_id: ObservedProjectionId,
}

impl ThreadingReadModelHandle {
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.projection_key
    }

    #[must_use]
    pub fn observer_id(&self) -> ObservedProjectionId {
        self.observer_id
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
pub fn open_threading_read_model(
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
    params: ThreadingReadModelParams,
) -> Option<ThreadingReadModelHandle> {
    let projection_key = threading_projection_key(&params.session_id)?;
    let registration_key =
        FrameworkProjectionKey::declared(projection_key.clone(), "projection.nmp.threading.graph")
            .ok()?;

    let projection = Arc::new(ThreadingProjection::etag(params.policy));
    let observer_id = app.open_observed_projection(ObservedProjection::from_shape(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        projection_key.clone(),
        params.scope.code(),
        params.shape,
        params.replay_limit,
    ));
    if observer_id.0 == 0 {
        return None;
    }

    let projection_for_typed = Arc::clone(&projection);
    let projection_key_for_typed = projection_key.clone();
    app.register_typed_snapshot_projection(registration_key, move || {
        Some(projection_for_typed.typed_projection(&projection_key_for_typed))
    });

    Some(ThreadingReadModelHandle {
        projection_key,
        observer_id,
    })
}

/// Close a threading read model opened by [`open_threading_read_model`].
pub fn close_threading_read_model(
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
    handle: ThreadingReadModelHandle,
) {
    app.close_observed_projection(handle.observer_id);
    app.remove_snapshot_projection(&handle.projection_key);
}
