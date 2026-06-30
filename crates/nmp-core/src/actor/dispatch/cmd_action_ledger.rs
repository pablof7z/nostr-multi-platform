//! `ActionLedgerCommand` family dispatch.

use crate::actor::ActionLedgerCommand;
use crate::relay::OutboundMessage;

use super::{cmd_publish, ActorContext};

pub(super) fn dispatch(
    cmd: ActionLedgerCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        ActionLedgerCommand::Ack(correlation_id) => {
            cmd_publish::ack_action_stage(correlation_id, ctx)
        }
        ActionLedgerCommand::RecordFailure {
            correlation_id,
            reason,
        } => cmd_publish::record_action_failure(correlation_id, reason, ctx),
        ActionLedgerCommand::RecordSuccess {
            correlation_id,
            result_json,
        } => cmd_publish::record_action_success(correlation_id, result_json, ctx),
    }
}
