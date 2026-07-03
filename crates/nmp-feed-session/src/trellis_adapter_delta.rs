use std::collections::BTreeMap;

use nmp_core::{DependentInterestChild, DependentInterestDelta, DependentInterestDeltaCommand};
use nmp_planner::InterestScope;
use trellis_core::{ResourceCommand, ResourceKey};

use crate::trellis_resources::{
    FeedSessionInterestScope, FeedSessionResourceCommand, InterestDemand,
};

#[derive(Default)]
pub(super) struct FeedSessionResourceLedger {
    active_children: BTreeMap<ResourceKey, DependentInterestChild>,
}

impl FeedSessionResourceLedger {
    pub(super) fn delta(
        &mut self,
        commands: &[ResourceCommand<FeedSessionResourceCommand>],
    ) -> DependentInterestDelta {
        let mut delta = DependentInterestDelta {
            commands: Vec::new(),
        };
        for command in commands {
            match command {
                ResourceCommand::Open { key, command, .. } => {
                    self.upsert_child(
                        &mut delta,
                        key,
                        child_from_command(command),
                        DependentInterestDeltaCommand::Open,
                    );
                }
                ResourceCommand::Replace { key, command, .. } => {
                    self.upsert_child(
                        &mut delta,
                        key,
                        child_from_command(command),
                        DependentInterestDeltaCommand::Replace,
                    );
                }
                ResourceCommand::Refresh { key, command, .. } => {
                    self.upsert_child(
                        &mut delta,
                        key,
                        child_from_command(command),
                        DependentInterestDeltaCommand::Refresh,
                    );
                }
                ResourceCommand::Close { key, .. } => {
                    if let Some(child) = self.active_children.remove(key) {
                        delta
                            .commands
                            .push(DependentInterestDeltaCommand::Close(child));
                    }
                }
            }
        }
        delta
    }

    fn upsert_child(
        &mut self,
        delta: &mut DependentInterestDelta,
        key: &ResourceKey,
        child: Option<DependentInterestChild>,
        command: impl FnOnce(DependentInterestChild) -> DependentInterestDeltaCommand,
    ) {
        let Some(child) = child else {
            if let Some(previous) = self.active_children.remove(key) {
                delta
                    .commands
                    .push(DependentInterestDeltaCommand::Close(previous));
            }
            return;
        };

        if let Some(previous) = self.active_children.insert(key.clone(), child.clone()) {
            if previous != child {
                delta
                    .commands
                    .push(DependentInterestDeltaCommand::Close(previous));
            }
        }
        delta.commands.push(command(child));
    }
}

fn child_from_command(command: &FeedSessionResourceCommand) -> Option<DependentInterestChild> {
    match command {
        FeedSessionResourceCommand::OpenInterest(demand) => Some(child_from_demand(demand)),
        FeedSessionResourceCommand::CloseInterest(_)
        | FeedSessionResourceCommand::ReplaceInterestSet(_)
        | FeedSessionResourceCommand::ReplayFromStore(_)
        | FeedSessionResourceCommand::AttachProjection(_)
        | FeedSessionResourceCommand::DetachProjection(_) => None,
    }
}

fn child_from_demand(demand: &InterestDemand) -> DependentInterestChild {
    DependentInterestChild::tailing(demand.shape.clone(), interest_scope(&demand.scope))
}

fn interest_scope(scope: &FeedSessionInterestScope) -> InterestScope {
    match scope {
        FeedSessionInterestScope::ActiveAccount => InterestScope::ActiveAccount,
        FeedSessionInterestScope::Account(pubkey) => InterestScope::Account(pubkey.clone()),
        FeedSessionInterestScope::Global => InterestScope::Global,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nmp_planner::{InterestLifecycle, InterestShape};
    use trellis_core::{Graph, ResourceKey, ResourcePlan};

    use super::*;
    use crate::trellis_resources::FeedSessionRouteProvenance;

    fn demand(author: &str) -> InterestDemand {
        InterestDemand::new(
            &InterestScope::ActiveAccount,
            InterestShape::timeline_for(
                BTreeSet::from([author.to_string()]),
                BTreeSet::from([1_u32]),
            ),
            InterestLifecycle::Tailing,
            FeedSessionRouteProvenance::ActiveFollowTimeline,
        )
    }

    fn authors(child: &DependentInterestChild) -> BTreeSet<String> {
        child.interest.shape.authors.clone()
    }

    fn test_scope() -> trellis_core::ScopeId {
        let mut graph = Graph::<FeedSessionResourceCommand>::new_with_command_type();
        let mut tx = graph.begin_transaction().unwrap();
        tx.create_scope("ledger-test").unwrap()
    }

    #[test]
    fn replace_with_changed_child_identity_closes_previous_child_first() {
        let scope = test_scope();
        let key = ResourceKey::new("stable-resource".to_string());
        let mut ledger = FeedSessionResourceLedger::default();
        let mut plan = ResourcePlan::new();
        plan.open(
            key.clone(),
            scope,
            FeedSessionResourceCommand::OpenInterest(demand("alice")),
        );
        let opened = ledger.delta(plan.commands());
        assert_eq!(opened.commands.len(), 1);

        let mut plan = ResourcePlan::new();
        plan.replace(
            key.clone(),
            scope,
            FeedSessionResourceCommand::OpenInterest(demand("bob")),
        );
        let replaced = ledger.delta(plan.commands());
        assert_eq!(replaced.commands.len(), 2);
        assert!(matches!(
            &replaced.commands[0],
            DependentInterestDeltaCommand::Close(child)
                if authors(child) == BTreeSet::from(["alice".to_string()])
        ));
        assert!(matches!(
            &replaced.commands[1],
            DependentInterestDeltaCommand::Replace(child)
                if authors(child) == BTreeSet::from(["bob".to_string()])
        ));

        let mut plan = ResourcePlan::new();
        plan.close(key, scope);
        let closed = ledger.delta(plan.commands());
        assert_eq!(closed.commands.len(), 1);
        assert!(matches!(
            &closed.commands[0],
            DependentInterestDeltaCommand::Close(child)
                if authors(child) == BTreeSet::from(["bob".to_string()])
        ));
    }
}
