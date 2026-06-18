//! Per-relay NOTICE log types for the `relay_diagnostics` projection.
//!
//! Extracted from `relay_diagnostics.rs` to keep that file under the 500-LOC
//! file-size gate (AGENTS.md).

use serde::Serialize;

/// One entry in the relay NOTICE log surfaced by `RelayDiagnosticsRow.notices`.
///
/// Populated from the bounded `RelayHealth.notices` / `RelayTransportStatus.notices`
/// ring (capped at 32 entries); ordered newest-first in the projection (the
/// raw ring is oldest-first; `build_relay_row` reverses on map). Shells render
/// `at_ms` via `relativeTimeFromUnixSeconds` at render time; `text` is pre-
/// truncated to 180 chars at the capture site in `relay_frame.rs`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::kernel) struct RelayDiagnosticsNotice {
    /// Wall-clock Unix epoch milliseconds when this NOTICE arrived.
    /// Shells format as "Xs ago" at render time.
    pub(in crate::kernel) at_ms: u64,
    /// Notice prose (truncated to 180 chars at the capture site).
    pub(in crate::kernel) text: String,
}
