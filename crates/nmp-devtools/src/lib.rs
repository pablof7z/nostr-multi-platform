//! Dev-only diagnostic receipt surface for NMP internals.
//!
//! This crate is intentionally not linked by runtime or app-facing crates. It
//! owns the X-Ray receipt vocabulary used by diagnostics tools: public receipts
//! are NMP-owned facts, while private adapters may read implementation
//! substrates such as Trellis to produce those facts.

#![forbid(unsafe_code)]

mod capsule;
mod feed_session;
mod prover;
mod receipt;
mod relay_correlation;
mod trellis;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

pub use capsule::{
    redact_receipts, XrayCapsule, XrayCapsuleProducer, XrayCapsuleVersions, XrayRedactionMode,
    XraySymbolicationEntry, XraySymbolicationManifest,
};
pub use feed_session::{
    receipts_from_feed_session_batch, XrayFeedSessionClock, XrayFeedSessionRecorder,
};
pub use prover::{XrayProbe, XrayReplaySession, XrayReplayTransaction, XrayScopeInventory};
pub use receipt::{
    XrayCauseLink, XrayCommandOutcome, XrayInterestDescriptor, XrayOutcomeStatus, XrayOwnerCounts,
    XrayProjectionContext, XrayReason, XrayReasonCode, XrayReasonParam, XrayReceipt,
    XrayReceiptEventKind, XrayReceiptRecorder, XrayReceiptStream, XrayRecordingConfig,
    XrayRelayEffect, XrayTeardownCascade, XrayTimestamp, XrayTransactionMarker,
};
pub use relay_correlation::{
    correlate_receipts_with_wire_subscriptions, XrayWireSubscriptionSnapshot,
};
pub use trellis::{receipts_from_trellis_commands, TrellisReceiptPayload};
