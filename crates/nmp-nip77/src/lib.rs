//! NIP-77 negentropy client support for NMP.
//!
//! This crate uses `nostr`'s NIP-77 client/relay message types for wire
//! framing, `negentropy` for reconciliation, and owns only the NMP-specific
//! session state. The kernel exposes generic substrate seams: an outbound REQ
//! interceptor and an inbound relay-text interceptor. Production composition
//! installs [`NegentropySyncRuntime`] through both seams.

#![forbid(unsafe_code)]

mod codec;
mod filter;
mod messages;
mod reconciler;
mod runtime;
#[cfg(test)]
mod runtime_tests;

pub use filter::{EligibleFilter, FilterEligibilityError};
pub use reconciler::{Reconciler, ReconcilerError, ReconcilerOutcome, SyncedItem};
pub use runtime::{NegentropySyncRuntime, RelayNegentropyState};

/// Frame-size cap used by the underlying negentropy engine.
pub const FRAME_SIZE_LIMIT: u64 = 64 * 1024;
