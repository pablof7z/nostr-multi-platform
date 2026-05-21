//! M4 — NIP-77 negentropy sync engine.
//!
//! Implements the cardinal doctrine **D2 — negentropy first, REQ second** by
//! providing the sync substrate that NMP's planner consults before issuing
//! REQ frames.  Transport-agnostic: the reconciler exchanges opaque byte
//! payloads; the relay protocol envelope (`NEG-OPEN`/`NEG-MSG`/`NEG-CLOSE`)
//! and the WebSocket wire are layered on top via [`wire`].
//!
//! ## Module map
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`reconciler`] | Wraps `negentropy::Negentropy` with a deterministic step API. |
//! | [`wire`] | Encodes / decodes NIP-77 client + relay frames as JSON arrays. |
//! | [`capability`] | Per-relay `supports_nip77` cache with a probe state machine. |
//! | [`capability_domain`] | `DomainModule` for persistent capability rows. |
//! | [`coverage_gate`] | Maps `Coverage` → `SyncStrategy` (skip REQ / REQ-since / sync-then-REQ). |
//! | [`planner_gate`] | Applies the coverage strategy in-place to a `CompiledPlan`. |
//! | [`triggers`] | Foreground / view-open-gap / relay-reconnect trigger fan-out. |
//! | [`metrics`] | Per-(filter, relay) `bytes_on_wire_via_neg` + `bytes_saved_vs_req` counters. |
//!
//! ## Doctrine
//!
//! * **D2** — negentropy first.  [`coverage_gate::decide_strategy`] returns
//!   [`SyncStrategy::SkipReq`] when the watermark is `CompleteAsOf` and fresh,
//!   [`SyncStrategy::NegThenReq`] when the relay supports NIP-77 and the gap is
//!   large, and [`SyncStrategy::ReqSince`] only as the explicit fallback path.
//! * **D6** — errors never cross FFI.  All public errors here are internal
//!   `Result` returns; mapping to `toast`/`busy` state happens at the actor
//!   boundary, not in the engine.
//! * **D8** — reconciliation is working-set-bounded.  The reconciler's frame
//!   budget is capped at `64 KiB` per step and the trigger engine deduplicates
//!   redundant work-items per `(filter_hash, relay_url)` before dispatching.

pub mod capability;
pub mod capability_domain;
pub mod coverage_gate;
pub mod metrics;
pub mod planner_gate;
pub mod reconciler;
pub mod triggers;
pub mod wire;

#[cfg(test)]
#[path = "reconciler_tests.rs"]
mod reconciler_tests;

#[cfg(test)]
#[path = "planner_gate_tests.rs"]
mod planner_gate_tests;

pub use capability::{
    CapabilityCache, CapabilityProbe, InMemoryCapabilityCache, ProbeOutcome, ProbeState,
    RelayCapabilities,
};
pub use capability_domain::{CapabilityDomain, CapabilityRow};
pub use coverage_gate::{decide_strategy, GateInputs, SyncStrategy, COVERAGE_THRESHOLD_PCT};
pub use metrics::{MetricsSnapshot, RelayFilterKey, SyncMetrics};
pub use planner_gate::{apply_coverage_filter, CoverageReport, GateDecision};
pub use reconciler::{
    Reconciler, ReconcilerError, ReconcilerOutcome, ReconcilerRole, SyncedItem,
};
pub use triggers::{ReconcileWork, TriggerEngine, TriggerEvent};
pub use wire::{ClientFrame, RelayFrame, WireError};

/// Frame-size cap shared by the reconciler + wire encoder.  64 KiB matches
/// the budget used by real NIP-77 relay implementations (strfry,
/// `relay-builder`) and keeps step latency bounded on mobile sockets — a
/// 64 KiB payload fits in roughly 8–10 WebSocket TCP segments on LTE/5G.
/// Smaller values inflate round-trip count without reducing bytes-on-wire
/// (most of which are 32-byte id payloads, not protocol overhead).  D8 —
/// working-set bounded.
pub const FRAME_SIZE_LIMIT: u64 = 64 * 1024;
