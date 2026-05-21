//! Actor scheduling policy helpers.

use std::time::Duration;

/// Maximum UI/runtime commands to process before yielding to relay + idle work.
pub(super) const COMMAND_DRAIN_BUDGET: usize = 64;

pub(super) struct CommandDrain {
    budget: usize,
    drained: usize,
}

impl CommandDrain {
    pub(super) fn new(budget: usize) -> Self {
        Self { budget, drained: 0 }
    }

    pub(super) fn can_drain_command(&self) -> bool {
        self.drained < self.budget
    }

    pub(super) fn record_command(&mut self) {
        self.drained = self.drained.saturating_add(1);
    }

    pub(super) fn hit_budget(&self) -> bool {
        self.drained >= self.budget
    }

    pub(super) fn relay_wait(&self, computed_wait: Duration) -> Duration {
        if self.hit_budget() {
            Duration::ZERO
        } else {
            computed_wait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lane_keeps_priority_until_budget_is_reached() {
        let mut drain = CommandDrain::new(3);
        assert!(drain.can_drain_command());

        drain.record_command();
        drain.record_command();
        assert!(drain.can_drain_command());
        assert_eq!(
            drain.relay_wait(Duration::from_millis(250)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn command_burst_yields_to_relay_and_idle_work_at_budget() {
        let mut drain = CommandDrain::new(2);
        drain.record_command();
        drain.record_command();

        assert!(!drain.can_drain_command());
        assert!(drain.hit_budget());
        assert_eq!(drain.relay_wait(Duration::from_millis(250)), Duration::ZERO);
    }

    #[test]
    fn oversized_budget_accounting_stays_saturated() {
        let mut drain = CommandDrain::new(1);
        drain.record_command();
        drain.record_command();

        assert!(drain.hit_budget());
        assert_eq!(drain.relay_wait(Duration::from_secs(1)), Duration::ZERO);
    }
}
