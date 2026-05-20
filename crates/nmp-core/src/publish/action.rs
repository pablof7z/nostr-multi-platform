//! `PublishAction` + `PublishModule` (the `ActionModule` impl).
//!
//! `start` is wired to the actor mailbox (M6): `ffi::action::execute_action`
//! validates a `PublishAction` through `ActionRegistry`, then converts a
//! `Publish` variant into `ActorCommand::PublishSignedEvent` for the actor
//! to publish. `reduce` is not yet driven — the publish engine drives
//! transitions in-process; feeding `RelayOk` / `Timeout` into `reduce`
//! lands with the durable action ledger.

use serde::{Deserialize, Serialize};

use crate::substrate::{
    ActionContext, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
    ActionTransition, SignedEvent,
};

/// Stable handle returned to the caller of `Publish`. Used to key snapshot
/// entries and to address the action in the ledger when M6 wires the ledger.
pub type PublishHandle = String;

/// Relay URL — grep-able alias so the `RelayDispatcher` shim can be swapped
/// for `nmp-nip01::RelayManager` from M8 without changing call sites. Single
/// crate-wide definition lives in `crate::relay`; re-exported here so
/// `publish` import paths are unchanged.
pub use crate::relay::RelayUrl;

/// Where a publish should go.
///
/// `Auto` defers to the `OutboxResolver` (NIP-65 + indexer fallback per D3).
/// `Explicit` is the named opt-out (D3: "manual relay selection is the
/// opt-out").
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishTarget {
    Auto,
    Explicit { relays: Vec<RelayUrl> },
}

/// The single public publish action.
///
/// The signed event is included pre-signed because the kernel ledger (M6) will
/// sign once via the active `IdentityModule` and then enqueue the publish — we
/// never re-sign on retry (per the M6 exit gate "re-publish of an event
/// preserves `id` and `sig`").
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PublishAction {
    Publish {
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
    },
    Cancel {
        handle: PublishHandle,
    },
}

/// Action ledger step — coarse-grained so the ledger can persist it without
/// knowing the engine's internal per-relay timing state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishStep {
    Planning,
    Dispatching,
    Waiting,
    Done,
}

/// Final outcome reported to the action ledger when the engine finishes.
///
/// `Mixed` covers the common case where some relays accepted and some
/// gave up — the snapshot carries the per-relay detail; the ledger gets a
/// single coarse verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishOutcome {
    Accepted {
        relays: Vec<RelayUrl>,
    },
    Mixed {
        accepted: Vec<RelayUrl>,
        failed: Vec<RelayUrl>,
    },
    FailedAfterRetries {
        failed: Vec<RelayUrl>,
    },
    NoTargets,
    Cancelled,
}

/// `ActionModule` impl. The runtime is the engine; this trait exists so the
/// ledger sees a uniform shape across actions.
pub struct PublishModule;

impl ActionModule for PublishModule {
    const NAMESPACE: &'static str = "nmp.publish";

    type Action = PublishAction;
    type Step = PublishStep;
    type Output = PublishOutcome;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        match action {
            PublishAction::Publish { event, .. } => {
                if event.id.is_empty() || event.sig.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "publish action requires a signed event with id+sig".to_string(),
                    ));
                }
                Ok(ActionPlan {
                    initial_step: PublishStep::Planning,
                    initial_status: ActionStatus::Pending,
                    deadline_ms: None,
                })
            }
            PublishAction::Cancel { handle } => {
                if handle.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "cancel requires a publish handle".to_string(),
                    ));
                }
                Ok(ActionPlan {
                    initial_step: PublishStep::Done,
                    initial_status: ActionStatus::Cancelled,
                    deadline_ms: None,
                })
            }
        }
    }

    fn reduce(
        _ctx: &mut ActionContext,
        _id: crate::substrate::ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        // The engine drives transitions in-process; the ledger merely
        // observes the coarse step. When M6 lands and the ledger feeds
        // RelayOk / Timeout into `reduce`, the bridge layer will translate
        // them into `PublishEngine::on_ack` calls and re-derive the step
        // from the engine. For now this is a deterministic pass-through so
        // the action module satisfies the trait without making promises the
        // engine can't keep.
        match input {
            ActionInput::Started => ActionTransition::Continue {
                step: PublishStep::Dispatching,
                status: ActionStatus::Running,
            },
            ActionInput::ResumedAfterRestart { step } => ActionTransition::Continue {
                step,
                status: ActionStatus::Running,
            },
            ActionInput::CapabilityResult { .. } => ActionTransition::Continue {
                step: PublishStep::Waiting,
                status: ActionStatus::Running,
            },
            ActionInput::RelayOk { .. } => ActionTransition::Continue {
                step: PublishStep::Waiting,
                status: ActionStatus::Running,
            },
            ActionInput::Timeout => ActionTransition::Fail {
                reason: "publish timed out before engine ack".to_string(),
                transient: true,
            },
            ActionInput::Cancel => ActionTransition::Complete {
                output: PublishOutcome::Cancelled,
            },
        }
    }
}
