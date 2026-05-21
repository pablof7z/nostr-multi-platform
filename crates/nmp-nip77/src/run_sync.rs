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

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
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
    /// Optional deadline in milliseconds since epoch. Part of the action's
    /// public input shape; consumed by the actor/reconciler layer that drives
    /// the sync — `start` is a pure validator and does not read it.
    pub deadline_ms: Option<u64>,
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

    fn start(
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn start_accepts_run_sync_action() {
        RunSync::start(
            &mut ctx(),
            RunSyncAction {
                targets: vec![("aa".into(), "wss://r/".into())],
                deadline_ms: Some(1_000),
            },
        )
        .expect("run sync action should be accepted");
    }
}
