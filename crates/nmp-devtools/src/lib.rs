//! Dev-only diagnostic receipt surface for NMP internals.
//!
//! This crate is intentionally not linked by runtime or app-facing crates. It
//! owns the X-Ray receipt vocabulary used by diagnostics tools: public receipts
//! are NMP-owned facts, while private adapters may read implementation
//! substrates such as Trellis to produce those facts.

#![forbid(unsafe_code)]

mod receipt;
mod trellis;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

pub use receipt::{
    XrayCauseLink, XrayCommandOutcome, XrayInterestDescriptor, XrayOutcomeStatus, XrayOwnerCounts,
    XrayProjectionContext, XrayReason, XrayReasonCode, XrayReasonParam, XrayReceipt,
    XrayReceiptEventKind, XrayReceiptRecorder, XrayReceiptStream, XrayRecordingConfig,
    XrayRelayEffect, XrayTeardownCascade, XrayTimestamp, XrayTransactionMarker,
};
pub use trellis::{receipts_from_trellis_commands, TrellisReceiptPayload};
