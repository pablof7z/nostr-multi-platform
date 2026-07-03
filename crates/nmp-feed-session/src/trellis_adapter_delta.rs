use std::collections::BTreeMap;

use nmp_core::{DependentInterestChild, DependentInterestDelta, DependentInterestDeltaCommand};
use nmp_planner::InterestScope;
use trellis_core::{ResourceCommand, ResourceKey};

use crate::diagnostics::{
    FeedSessionDiagnosticEventKind, FeedSessionDiagnosticInterest,
    FeedSessionDiagnosticOwnerCounts, FeedSessionDiagnosticReceipt,
};
use crate::trellis_resources::{
    digest, lifecycle_part, FeedSessionInterestScope, FeedSessionResourceCommand, InterestDemand,
};

#[derive(Default)]
pub(super) struct FeedSessionResourceLedger {
    active_children: BTreeMap<ResourceKey, DependentInterestChild>,
}

pub(super) struct FeedSessionResourceLedgerOutput {
    pub(super) delta: DependentInterestDelta,
    pub(super) diagnostics: Vec<FeedSessionDiagnosticReceipt>,
}

impl FeedSessionResourceLedger {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ledger unit tests exercise the no-diagnostics path; production uses delta_with_diagnostics"
        )
    )]
    pub(super) fn delta(
        &mut self,
        commands: &[ResourceCommand<FeedSessionResourceCommand>],
    ) -> DependentInterestDelta {
        self.delta_with_diagnostics(commands, false).delta
    }

    pub(super) fn delta_with_diagnostics(
        &mut self,
        commands: &[ResourceCommand<FeedSessionResourceCommand>],
        diagnostics_enabled: bool,
    ) -> FeedSessionResourceLedgerOutput {
        let mut delta = DependentInterestDelta {
            commands: Vec::new(),
        };
        let mut diagnostics = Vec::new();
        for command in commands {
            let before = owner_count(self.active_children.contains_key(command.key()));
            let diagnostic_event = diagnostic_event_kind(command);
            let diagnostic_interest = diagnostic_interest(command);
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
            if diagnostics_enabled {
                let after = owner_count(self.active_children.contains_key(command.key()));
                diagnostics.push(FeedSessionDiagnosticReceipt {
                    event: diagnostic_event,
                    resource_id: command.key().as_str().to_string(),
                    interest: diagnostic_interest,
                    owner_counts: FeedSessionDiagnosticOwnerCounts::known(before, after),
                });
            }
        }
        FeedSessionResourceLedgerOutput { delta, diagnostics }
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

fn diagnostic_event_kind(
    command: &ResourceCommand<FeedSessionResourceCommand>,
) -> FeedSessionDiagnosticEventKind {
    match command {
        ResourceCommand::Open { .. } => FeedSessionDiagnosticEventKind::Open,
        ResourceCommand::Replace { .. } => FeedSessionDiagnosticEventKind::Replace,
        ResourceCommand::Refresh { .. } => FeedSessionDiagnosticEventKind::Refresh,
        ResourceCommand::Close { .. } => FeedSessionDiagnosticEventKind::Close,
    }
}

fn diagnostic_interest(
    command: &ResourceCommand<FeedSessionResourceCommand>,
) -> Option<FeedSessionDiagnosticInterest> {
    match command {
        ResourceCommand::Open { command, .. }
        | ResourceCommand::Replace { command, .. }
        | ResourceCommand::Refresh { command, .. } => interest_from_command(command),
        ResourceCommand::Close { .. } => None,
    }
}

fn interest_from_command(
    command: &FeedSessionResourceCommand,
) -> Option<FeedSessionDiagnosticInterest> {
    match command {
        FeedSessionResourceCommand::OpenInterest(demand) => Some(interest_from_demand(demand)),
        FeedSessionResourceCommand::CloseInterest(_)
        | FeedSessionResourceCommand::ReplaceInterestSet(_)
        | FeedSessionResourceCommand::ReplayFromStore(_)
        | FeedSessionResourceCommand::AttachProjection(_)
        | FeedSessionResourceCommand::DetachProjection(_) => None,
    }
}

fn interest_from_demand(demand: &InterestDemand) -> FeedSessionDiagnosticInterest {
    let interest_key = demand.resource_key();
    FeedSessionDiagnosticInterest::new(
        interest_key.as_str().to_string(),
        demand.scope.key_part(),
        format!(
            "lifecycle={}:shape={}",
            lifecycle_part(&demand.lifecycle),
            digest(("interest-shape", &demand.shape))
        ),
        demand.provenance.key_part(),
    )
}

fn owner_count(active: bool) -> u32 {
    u32::from(active)
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
