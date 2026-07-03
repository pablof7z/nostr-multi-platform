//! `ActionLedgerCommand` family dispatch.

use crate::actor::ActionLedgerCommand;
use crate::relay::OutboundMessage;

use super::ActorContext;

pub(super) fn dispatch(
    cmd: ActionLedgerCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        ActionLedgerCommand::Ack(correlation_id) => ack_action_stage(correlation_id, ctx),
        ActionLedgerCommand::RecordFailure {
            correlation_id,
            reason,
        } => record_action_failure(correlation_id, reason, ctx),
        ActionLedgerCommand::RecordSuccess {
            correlation_id,
            result_json,
        } => record_action_success(correlation_id, result_json, ctx),
    }
}

fn record_action_failure(
    correlation_id: String,
    reason: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Writes `Failed { reason }` to `action_stages` and a terminal
    // verdict to `action_results` so off-thread failures clear host spinners.
    ctx.kernel.record_action_failure(correlation_id, reason);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

fn record_action_success(
    correlation_id: String,
    result_json: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    ctx.kernel
        .record_action_success(correlation_id, result_json);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

fn ack_action_stage(
    correlation_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    ctx.kernel.ack_action_stage(&correlation_id);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}
