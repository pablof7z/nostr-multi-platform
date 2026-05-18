//! `RunSync` — manual reconciliation action.
//!
//! Implements [`ActionModule`] so apps can wire a "sync now" button to the
//! M4 engine without inventing per-relay surface area.  The action accepts a
//! list of `(filter_hash, relay_url)` targets, runs them through the
//! reconciler, and emits a [`RunSyncOutput`] summary.
//!
//! The reduce step here is *thin* — the real work happens at the planner /
//! actor layer where the bytes actually fly across the wire.  This module is
//! the orchestration shell; it lets the action surface a `busy` flag and a
//! `toast` field per D6 without hand-rolling action plumbing in every app.

use nmp_core::substrate::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection,
    ActionStatus, ActionTransition,
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
    type Output = RunSyncOutput;

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

    fn reduce(
        _ctx: &mut ActionContext,
        _id: ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        match input {
            ActionInput::Started => ActionTransition::Continue {
                step: RunSyncStep::Running {
                    remaining: 0,
                    completed: 0,
                },
                status: ActionStatus::Running,
            },
            ActionInput::ResumedAfterRestart { step } => ActionTransition::Continue {
                step,
                status: ActionStatus::Running,
            },
            ActionInput::CapabilityResult { value } => match parse_progress(&value) {
                Ok(progress) => {
                    if progress.remaining == 0 {
                        ActionTransition::Complete {
                            output: RunSyncOutput {
                                completed: progress.completed,
                                bytes_on_wire_via_neg: progress
                                    .bytes_on_wire_via_neg
                                    .unwrap_or(0),
                                bytes_saved_vs_req: progress.bytes_saved_vs_req.unwrap_or(0),
                            },
                        }
                    } else {
                        ActionTransition::Continue {
                            step: RunSyncStep::Running {
                                remaining: progress.remaining,
                                completed: progress.completed,
                            },
                            status: ActionStatus::Running,
                        }
                    }
                }
                Err(err) => ActionTransition::Fail {
                    reason: format!("malformed capability payload: {err}"),
                    transient: false,
                },
            },
            ActionInput::RelayOk { .. } => ActionTransition::Continue {
                step: RunSyncStep::Running {
                    remaining: 0,
                    completed: 0,
                },
                status: ActionStatus::Running,
            },
            ActionInput::Timeout => ActionTransition::Fail {
                reason: "deadline exceeded".into(),
                transient: true,
            },
            ActionInput::Cancel => ActionTransition::Fail {
                reason: "cancelled by user".into(),
                transient: false,
            },
        }
    }
}

#[derive(Deserialize)]
struct ProgressPayload {
    completed: u32,
    remaining: u32,
    bytes_on_wire_via_neg: Option<u64>,
    bytes_saved_vs_req: Option<u64>,
}

fn parse_progress(value: &serde_json::Value) -> Result<ProgressPayload, String> {
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}

/// Helper for tests / actor: finalise a `Running` step into an output.
pub fn finalise(
    completed: u32,
    bytes_on_wire_via_neg: u64,
    bytes_saved_vs_req: u64,
) -> ActionTransition<RunSyncStep, RunSyncOutput> {
    ActionTransition::Complete {
        output: RunSyncOutput {
            completed,
            bytes_on_wire_via_neg,
            bytes_saved_vs_req,
        },
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

    #[test]
    fn started_input_transitions_to_running() {
        let next = RunSync::reduce(&mut ctx(), "id".into(), ActionInput::Started);
        match next {
            ActionTransition::Continue { status, .. } => {
                assert_eq!(status, ActionStatus::Running);
            }
            other => panic!("expected Continue, got {other:?}"),
        }
    }

    #[test]
    fn capability_progress_decrements_remaining() {
        let value = serde_json::json!({"completed": 2, "remaining": 1});
        let next = RunSync::reduce(
            &mut ctx(),
            "id".into(),
            ActionInput::CapabilityResult { value },
        );
        match next {
            ActionTransition::Continue {
                step: RunSyncStep::Running {
                    remaining,
                    completed,
                },
                ..
            } => {
                assert_eq!(remaining, 1);
                assert_eq!(completed, 2);
            }
            other => panic!("expected Running continue, got {other:?}"),
        }
    }

    #[test]
    fn final_progress_payload_completes_action() {
        let value = serde_json::json!({
            "completed": 5,
            "remaining": 0,
            "bytes_on_wire_via_neg": 2048,
            "bytes_saved_vs_req": 32_768
        });
        let next = RunSync::reduce(
            &mut ctx(),
            "id".into(),
            ActionInput::CapabilityResult { value },
        );
        match next {
            ActionTransition::Complete { output } => {
                assert_eq!(output.completed, 5);
                assert_eq!(output.bytes_on_wire_via_neg, 2_048);
                assert_eq!(output.bytes_saved_vs_req, 32_768);
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn timeout_input_fails_transiently() {
        let next = RunSync::reduce(&mut ctx(), "id".into(), ActionInput::Timeout);
        match next {
            ActionTransition::Fail { transient, .. } => assert!(transient),
            other => panic!("expected transient Fail, got {other:?}"),
        }
    }

    #[test]
    fn finalise_produces_output() {
        let t = finalise(3, 1_024, 8_192);
        match t {
            ActionTransition::Complete { output } => {
                assert_eq!(output.completed, 3);
                assert_eq!(output.bytes_on_wire_via_neg, 1_024);
                assert_eq!(output.bytes_saved_vs_req, 8_192);
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }
}
