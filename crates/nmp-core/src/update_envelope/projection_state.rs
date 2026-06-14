//! ADR-0055 Rung 2 — wire presence state for one typed projection sidecar entry.
//!
//! Extracted to a sibling module so `update_envelope.rs` stays under the 500-LOC
//! ceiling. The enum + both `From` conversions are the entire owned↔FlatBuffers
//! bridge for the `state:ProjectionPresenceState` wire field.

use crate::transport::wire as fb;

/// Owned form of `nmp.transport.ProjectionPresenceState` — the per-projection
/// presence classification carried alongside `projection_rev` on each sidecar.
///
/// Two states appear on the wire in Rung 2:
/// - `Changed`: rev advanced; payload authoritative.
/// - `Cleared`: projection went absent (account-switch / interest closed);
///   payload absent; host MUST drop its cached value.
///
/// The third logical state — `Unchanged` (rev did not advance; payload omitted;
/// host reuses its prior decoded value) — is a Rung-3 concept. In Rung 2 every
/// projection is still emitted each tick, so only `Changed` and `Cleared`
/// appear on the wire. The enum is defined now so Rung 3 needs no wire change.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum WireProjectionState {
    /// Rev advanced; payload authoritative. Default for old (pre-Rung-2) frames.
    #[default]
    Changed,
    /// Projection went absent; host drops its cached value.
    Cleared,
}

impl From<fb::ProjectionPresenceState> for WireProjectionState {
    fn from(v: fb::ProjectionPresenceState) -> Self {
        if v == fb::ProjectionPresenceState::Cleared {
            Self::Cleared
        } else {
            Self::Changed
        }
    }
}

impl From<WireProjectionState> for fb::ProjectionPresenceState {
    fn from(v: WireProjectionState) -> Self {
        match v {
            WireProjectionState::Changed => fb::ProjectionPresenceState::Changed,
            WireProjectionState::Cleared => fb::ProjectionPresenceState::Cleared,
        }
    }
}
