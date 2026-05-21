//! `RunSync` — manual reconciliation action.
//!
//! Implements [`ActionModule`] so apps can wire a "sync now" button to the
//! M4 engine without inventing per-relay surface area.  The action accepts a
//! list of `(filter_hash, relay_url)` targets and hands them to the
//! reconciler.
//!
//! The real work happens at the planner / actor layer where the bytes
//! actually fly across the wire.  This module is the orchestration shell;
//! it lets the action surface a `busy` flag and a `toast` field per D6
//! without hand-rolling action plumbing in every app.

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPlan, ActionRejection, ActionStatus,
};
use serde::{Deserialize, Serialize};

/// Action namespace; surfaced as the public `NAMESPACE` constant on
/// [`RunSync`] for the codegen tool.
pub const ACTION_NAMESPACE: &str = "nmp.nip77.run_sync";

/// What the user requests when invoking `RunSync`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunSyncAction {
    /// `(filter_hash_hex, relay_url)` targets.  Empty list ⇒ reconcile every
    /// open pair the trigger engine knows about (the actor expands this).
    pub targets: Vec<(String, String)>,
    /// Optional deadline in milliseconds since epoch.
    pub deadline_ms: Option<u64>,
}

/// Steps the action transitions through.  Stored as a small enum so a
/// crashing app can resume from disk without losing more than one step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum RunSyncStep {
    Prepared { remaining: u32 },
    Running { remaining: u32, completed: u32 },
    Finalising,
}

/// Final summary the action emits on success.
///
/// Surfaced on the snapshot-projection (pull) path by the actor once the
/// reconciler reports a terminal verdict.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunSyncOutput {
    pub completed: u32,
    pub bytes_on_wire_via_neg: u64,
    pub bytes_saved_vs_req: u64,
}

/// `ActionModule` implementation.
pub struct RunSync;

impl ActionModule for RunSync {
    const NAMESPACE: &'static str = ACTION_NAMESPACE;

    type Action = RunSyncAction;
    type Step = RunSyncStep;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let target_count = action.targets.len() as u32;
        Ok(ActionPlan {
            initial_step: RunSyncStep::Prepared {
                remaining: target_count,
            },
            initial_status: ActionStatus::Pending,
            deadline_ms: action.deadline_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn start_returns_pending_plan() {
        let plan = RunSync::start(
            &mut ctx(),
            RunSyncAction {
                targets: vec![("aa".into(), "wss://r/".into())],
                deadline_ms: Some(1_000),
            },
        )
        .unwrap();
        assert_eq!(plan.initial_status, ActionStatus::Pending);
        assert!(matches!(
            plan.initial_step,
            RunSyncStep::Prepared { remaining: 1 }
        ));
        assert_eq!(plan.deadline_ms, Some(1_000));
    }
}
