//! Publishing pipeline (M7 substrate, scaffolded ahead of M3 / M6 / M8).
//!
//! This module owns the substrate-level publish engine: the action shape, the
//! per-(event, relay) state machine, the durable retry queue contract, and the
//! `PublishStatusView` payload. The kernel actor and relay-manager wiring land
//! when their respective milestones ship (#43 Signer, #46 RelayManager, M3
//! LMDB persistence). Until then the engine consumes minimal trait shims that
//! the downstream milestones will satisfy without forcing the publish
//! pipeline to be rewritten — see `traits.rs`.
//!
//! Doctrine map:
//! - D3 (outbox automatic): `PublishTarget::Auto` resolves via `OutboxResolver`
//!   — the engine never picks relays from a hardcoded constant.
//! - D4 (single writer per fact): per-(event, relay) status is owned by the
//!   engine; the snapshot is derived from it.
//! - D5 (snapshots bounded by what's open): the view payload is small and
//!   only carries currently-pending plus a bounded recent window.
//! - D6 (errors never cross FFI as exceptions): per-relay publish failures
//!   surface as `RecentFailure` entries on the snapshot plus a coarse
//!   `PublishOutcome::Mixed` / `FailedAfterRetries` on the action ledger.
//!   Engine-level `PublishEngineError` values (`DuplicateHandle`, `NoTargets`,
//!   `Store`) are mapped by `engine::engine_error_to_failure` into the same
//!   `RecentFailure` shape so the FFI boundary only ever sees state, never
//!   an exception or `Result<T, E>`.
//! - D7 (capabilities report): the `RelayDispatcher` shim returns purely
//!   descriptive `RelayAck { ok, code, message, details }` values; the
//!   engine's `classify_ack` (in `state.rs`) is the only place that maps
//!   ack codes to retry policy.
//! - D8 (≤60 Hz/view): the view payload exposes a monotonic `rev` so the
//!   projection bridge can coalesce publish-status changes under the view
//!   emission budget.

mod action;
mod engine;
mod nip65;
mod state;
#[cfg(test)]
mod tests;
mod traits;
mod view;

pub use action::{
    PublishAction, PublishHandle, PublishModule, PublishOutcome, PublishStep, PublishTarget,
    RelayUrl,
};
pub use engine::{
    engine_error_to_failure, outcome_of, PublishEngine, PublishEngineError, TerminalOutcome,
    ENGINE_FAILURE_RELAY_URL,
};
pub use nip65::Nip65OutboxResolver;
pub use state::{PerRelayState, PublishAttempt, RelayAck, RelayPlan, RetryPolicy, RetryVerdict};
pub use traits::{
    InMemoryPublishStore, NoopOutboxResolver, NoopSigner, OutboxResolver, PublishRecord,
    PublishStore, PublishStoreError, QueueDispatcher, RelayDispatcher, ReplayDispatcher, Signer,
    SignerError, StaticOutbox,
};
pub use view::{
    EventPublishStatus, PublishStatusSnapshot, PublishStatusSpec, PublishStatusView, RecentFailure,
    RecentSuccess,
};
