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
//! - D6 (errors never cross FFI as exceptions): publish failures are represented
//!   in `RecentFailure` entries and coarse `PublishOutcome` values.
//! - D7 (capabilities report): the `RelayDispatcher` shim returns raw
//!   transport results (`RelayAck`); the engine decides the policy.
//! - D8 (≤60 Hz/view): the view payload exposes a monotonic `rev` so the
//!   projection bridge can coalesce publish-status changes under the view
//!   emission budget.

mod action;
mod engine;
mod state;
#[cfg(test)]
mod tests;
mod traits;
mod view;

pub use action::{
    PublishAction, PublishHandle, PublishModule, PublishOutcome, PublishStep, PublishTarget,
    RelayUrl,
};
pub use engine::{outcome_of, PublishEngine, PublishEngineError};
pub use state::{
    AckClass, PerRelayState, PublishAttempt, RelayAck, RelayPlan, RetryPolicy, RetryVerdict,
};
pub use traits::{
    InMemoryPublishStore, NoopOutboxResolver, NoopSigner, OutboxResolver, PublishRecord,
    PublishStore, PublishStoreError, RelayDispatcher, ReplayDispatcher, Signer, SignerError,
    StaticOutbox,
};
pub use view::{
    EventPublishStatus, PublishStatusSnapshot, PublishStatusSpec, PublishStatusView, RecentFailure,
    RecentSuccess,
};
