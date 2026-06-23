//! Native actor relay-control state.

use std::time::Instant;

use nmp_network::pool::RelayHandle;

use crate::relay::RelayRole;

/// One per-URL relay-worker handle. T105: `relay_url` (NOT `role`) is the
/// pool key — every resolved write/read relay gets its own socket. `role`
/// is retained so the actor can route diagnostic-bucket updates back to
/// the kernel's lane-keyed `RelayHealth` rows until per-URL health lands (M11).
///
/// Phase F: `handle` is the generational [`RelayHandle`] handed back by
/// `Pool::ensure_open_with_role`; outbound frames go through
/// `pool.send(handle, WireFrame::Text(..))` and shutdown is `pool.close(handle)`.
/// The per-actor `generation` counter is unrelated to `handle.generation()`
/// (the pool's slot generation) — it's a strictly-monotonic stamp the actor
/// uses to drop in-flight events from prior `ensure_open` rounds.
#[cfg(feature = "native")]
pub(in crate::actor) struct RelayControl {
    /// Strictly-monotonic per-actor stamp assigned at `ensure_relay_worker`
    /// time. Phase F: no longer the worker-side generation; kept as a
    /// diagnostic field for the FFI surface and spawn-order tests.
    #[allow(dead_code)]
    pub(super) generation: u64,
    #[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.
    pub(super) role: RelayRole,
    #[allow(dead_code)] // The URL this worker dials — the routing key in the pool.
    pub(super) relay_url: String,
    pub(super) handle: RelayHandle,
    pub(super) connection_kind: RelayConnectionKind,
    pub(super) idle_since: Option<Instant>,
}

#[cfg(feature = "native")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::actor) enum RelayConnectionKind {
    Persistent,
    Temporary,
}
