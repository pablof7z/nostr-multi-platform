//! One shared fail-closed `ActorCommand` shape for this crate.
//!
//! Both the backend-selection layer (`selector.rs`, before any backend is
//! reached) and each backend's own pre-dispatch validation (e.g.
//! `backend::cashu`'s "no active account"/"unsupported mint" checks) need
//! the identical shape: a structured `ShowErrorToken` plus, when a
//! `correlation_id` was supplied, a paired `ActionLedger::RecordFailure`.
//! One canonical implementation — not two copies drifting independently.

use nmp_core::actor::{ActionLedgerCommand, ActorCommand};
use nmp_core::ui_token::UiToken;

/// Fail-closed before any value-moving/dispatched work happens.
pub(crate) fn fail_closed(
    code: &'static str,
    correlation_id: Option<String>,
    reason: String,
) -> Vec<ActorCommand> {
    let mut out = vec![ActorCommand::ShowErrorToken {
        token: UiToken::error(code, reason.clone()),
    }];
    if let Some(id) = correlation_id {
        out.push(ActorCommand::ActionLedger(
            ActionLedgerCommand::RecordFailure {
                correlation_id: id,
                reason,
            },
        ));
    }
    out
}
