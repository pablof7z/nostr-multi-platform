use std::collections::VecDeque;

use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSendStatus, CommandSender, DependentInterestDelta};
use nmp_feed::TeardownAction;

#[derive(Default)]
pub(super) struct FeedSessionDeliveryQueue {
    pending_deltas: VecDeque<PendingInterestDelta>,
    pending_output_clear: Option<TeardownAction>,
}

pub(super) struct FeedSessionDeliveryFlush {
    pub(super) delivered_delta: bool,
    pub(super) output_clear: Option<TeardownAction>,
}

#[derive(Clone)]
struct PendingInterestDelta {
    delta: DependentInterestDelta,
    reason: String,
}

impl FeedSessionDeliveryQueue {
    pub(super) fn push_delta(&mut self, delta: DependentInterestDelta, reason: &'static str) {
        if delta.is_empty() {
            return;
        }
        self.pending_deltas.push_back(PendingInterestDelta {
            delta,
            reason: reason.to_string(),
        });
    }

    pub(super) fn push_output_clear(&mut self, action: TeardownAction) {
        if self.pending_output_clear.is_none() {
            self.pending_output_clear = Some(action);
        }
    }

    pub(super) fn flush(
        &mut self,
        sender: &CommandSender,
        owner: SubOwnerKey,
    ) -> FeedSessionDeliveryFlush {
        let mut delivered_delta = false;
        while let Some(pending) = self.pending_deltas.front().cloned() {
            let command = ActorCommand::Interests(InterestsCommand::ApplyDependentInterestDelta {
                owner,
                delta: pending.delta,
                reason: pending.reason,
            });
            match sender.send(command) {
                Ok(CommandSendStatus::Enqueued) => {
                    self.pending_deltas.pop_front();
                    delivered_delta = true;
                }
                Ok(CommandSendStatus::DroppedFull) | Err(_) => {
                    return FeedSessionDeliveryFlush {
                        delivered_delta,
                        output_clear: None,
                    };
                }
            }
        }
        FeedSessionDeliveryFlush {
            delivered_delta,
            output_clear: self.pending_output_clear.take(),
        }
    }
}
