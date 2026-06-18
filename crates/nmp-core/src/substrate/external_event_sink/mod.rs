//! Kernel-owned external event sink — typed frames, policy trait, and
//! supporting enumerations.
//!
//! This module replaces the raw-event tap escape hatch (see
//! `docs/escape-hatches.md`). The new seam has two advantages over the old raw
//! event tap mechanism:
//!
//! 1. **One serialization** — `canonical_json` is produced once at the
//!    dispatch site and shared (via `Arc<str>`) by every destination.
//! 2. **Off actor-thread delivery** — the dispatcher owns a bounded channel
//!    and a single worker thread; policy resolution and `Pool::send` happen
//!    there, not on the actor thread.
//!
//! # Migration
//!
//! The `RawEventObserver` escape hatch has been fully removed. This seam is the
//! canonical replacement for in-process relay forwarding. External per-event
//! consumers (e.g. an out-of-tree nostrdb mirror) read through the store via a
//! bounded pull cursor (forthcoming), never a kernel push sink. See
//! `docs/escape-hatches.md`.

pub mod diagnostics;
pub mod dispatcher;
mod worker;

use std::sync::Arc;

use crate::store::RawEvent;
use crate::substrate::RawEventForwardTarget;

// ─── KindFilter re-export ─────────────────────────────────────────────────────
//
// Re-exported here so `ExternalEventSinkPolicy::kind_filter` names it without
// depending on the actor module.
pub use crate::actor::KindFilter;

// ─── IngestOutcomeKind ────────────────────────────────────────────────────────

/// The ingest outcome that triggered this frame.
///
/// Inserted | Replaced | Duplicate | Ephemeral — **Duplicate is included**.
/// This is the invariant the design doc §a calls out: the duplicate live
/// raw fan-out with source-relay provenance is the one capability
/// `IngestParser` does NOT cover.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestOutcomeKind {
    Inserted,
    Replaced,
    /// Re-delivered by another relay; source-relay provenance differs.
    /// **Must NOT be dropped** — the duplicate live fan-out with source-relay
    /// provenance is the capability `IngestParser` does not cover.
    Duplicate,
    Ephemeral,
}

impl IngestOutcomeKind {
    /// Convert from [`crate::store::InsertOutcome`].
    /// Returns `None` for outcomes that must NOT trigger dispatch
    /// (`Rejected`, `Superseded`, `Tombstoned`).
    #[must_use]
    pub fn from_insert_outcome(outcome: &crate::store::InsertOutcome) -> Option<Self> {
        use crate::store::InsertOutcome;
        match outcome {
            InsertOutcome::Inserted { .. } => Some(Self::Inserted),
            InsertOutcome::Replaced { .. } => Some(Self::Replaced),
            InsertOutcome::Duplicate { .. } => Some(Self::Duplicate),
            InsertOutcome::Ephemeral { .. } => Some(Self::Ephemeral),
            _ => None,
        }
    }
}

// ─── SignedEventFrame ─────────────────────────────────────────────────────────

/// A fully-verified inbound signed event produced at the ingest chokepoint.
///
/// `canonical_json` is the flat NIP-01 object
/// `{id,pubkey,created_at,kind,tags,content,sig}` serialized **once** and
/// then shared across every destination.  No consumer should re-parse it
/// into a `RawEvent`; use the `raw` field directly.
#[derive(Clone, Debug)]
pub struct SignedEventFrame {
    /// The typed, verified event. `Arc` avoids per-destination copying.
    pub raw: Arc<RawEvent>,
    /// Verbatim NIP-01 JSON, serialized once.
    pub canonical_json: Arc<str>,
    /// The relay URL that delivered the event (from store provenance).
    pub source_relay: Option<Arc<str>>,
    /// The store outcome that caused this frame to be emitted.
    pub ingest_outcome: IngestOutcomeKind,
}

impl SignedEventFrame {
    /// Construct a frame; serializes `raw` to JSON exactly once.
    ///
    /// Returns `None` on serialization failure (D6 — best-effort silent drop).
    #[must_use]
    pub fn build(
        raw: Arc<RawEvent>,
        source_relay: Option<Arc<str>>,
        ingest_outcome: IngestOutcomeKind,
    ) -> Option<Self> {
        let canonical_json: Arc<str> = serde_json::to_string(&*raw).ok()?.into();
        Some(Self {
            raw,
            canonical_json,
            source_relay,
            ingest_outcome,
        })
    }
}

// ─── SinkDestination ─────────────────────────────────────────────────────────

/// Where the dispatcher should deliver a [`SignedEventFrame`].
#[derive(Clone, Debug)]
pub enum SinkDestination {
    /// Build `["EVENT", <canonical_json>]` and send to a relay via `Pool`.
    Relay(RawEventForwardTarget),
}

// ─── ExternalEventSinkPolicy ─────────────────────────────────────────────────

/// Injected policy: decides whether and where to deliver a `SignedEventFrame`.
///
/// This is the single canonical policy seam for external event delivery.
/// Implementations decide which relay targets (or native sinks) should
/// receive each inbound signed event frame.
pub trait ExternalEventSinkPolicy: Send + Sync {
    /// Subset of event kinds this policy wants to observe.
    /// Called once per registration; the dispatcher gates on this before
    /// calling `destinations`.
    fn kind_filter(&self) -> KindFilter;

    /// Resolve delivery destinations for `frame`.
    /// An empty `Vec` means "do not forward".
    fn destinations(&self, frame: &SignedEventFrame) -> Vec<SinkDestination>;
}
