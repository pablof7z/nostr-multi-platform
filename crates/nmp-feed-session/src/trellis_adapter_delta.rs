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
    active_children: BTreeMap<ResourceKey, ActiveChild>,
}

pub(super) struct FeedSessionResourceLedgerOutput {
    pub(super) delta: DependentInterestDelta,
    pub(super) diagnostics: Vec<FeedSessionDiagnosticReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveChild {
    child: DependentInterestChild,
    scope: String,
    provenance: &'static str,
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
            let before = diagnostics_enabled
                .then(|| owner_count(self.active_children.contains_key(command.key())));
            let diagnostic_event = diagnostics_enabled.then(|| diagnostic_event_kind(command));
            let mut diagnostic_interest_record = None;
            match command {
                ResourceCommand::Open { key, command, .. } => {
                    let child = active_child_from_command(command);
                    if diagnostics_enabled {
                        diagnostic_interest_record =
                            child.as_ref().map(|child| diagnostic_interest(key, child));
                    }
                    self.upsert_child(&mut delta, key, child, DependentInterestDeltaCommand::Open);
                }
                ResourceCommand::Replace { key, command, .. } => {
                    let child = active_child_from_command(command);
                    if diagnostics_enabled {
                        diagnostic_interest_record =
                            child.as_ref().map(|child| diagnostic_interest(key, child));
                    }
                    self.upsert_child(
                        &mut delta,
                        key,
                        child,
                        DependentInterestDeltaCommand::Replace,
                    );
                }
                ResourceCommand::Refresh { key, command, .. } => {
                    let child = active_child_from_command(command);
                    if diagnostics_enabled {
                        diagnostic_interest_record =
                            child.as_ref().map(|child| diagnostic_interest(key, child));
                    }
                    self.upsert_child(
                        &mut delta,
                        key,
                        child,
                        DependentInterestDeltaCommand::Refresh,
                    );
                }
                ResourceCommand::Close { key, .. } => {
                    if diagnostics_enabled {
                        diagnostic_interest_record = self
                            .active_children
                            .get(key)
                            .map(|child| diagnostic_interest(key, child));
                    }
                    if let Some(child) = self.active_children.remove(key) {
                        delta
                            .commands
                            .push(DependentInterestDeltaCommand::Close(child.child));
                    }
                }
            }
            if diagnostics_enabled {
                let after = owner_count(self.active_children.contains_key(command.key()));
                diagnostics.push(FeedSessionDiagnosticReceipt {
                    event: diagnostic_event.expect("diagnostic event set when enabled"),
                    resource_id: command.key().as_str().to_string(),
                    interest: diagnostic_interest_record,
                    owner_counts: FeedSessionDiagnosticOwnerCounts::known(
                        before.expect("diagnostic before count set when enabled"),
                        after,
                    ),
                });
            }
        }
        FeedSessionResourceLedgerOutput { delta, diagnostics }
    }

    fn upsert_child(
        &mut self,
        delta: &mut DependentInterestDelta,
        key: &ResourceKey,
        child: Option<ActiveChild>,
        command: impl FnOnce(DependentInterestChild) -> DependentInterestDeltaCommand,
    ) {
        let Some(child) = child else {
            if let Some(previous) = self.active_children.remove(key) {
                delta
                    .commands
                    .push(DependentInterestDeltaCommand::Close(previous.child));
            }
            return;
        };

        if let Some(previous) = self.active_children.insert(key.clone(), child.clone()) {
            if previous.child != child.child {
                delta
                    .commands
                    .push(DependentInterestDeltaCommand::Close(previous.child));
            }
        }
        delta.commands.push(command(child.child));
    }
}

fn active_child_from_command(command: &FeedSessionResourceCommand) -> Option<ActiveChild> {
    match command {
        FeedSessionResourceCommand::OpenInterest(demand) => Some(active_child_from_demand(demand)),
        FeedSessionResourceCommand::CloseInterest(_)
        | FeedSessionResourceCommand::ReplaceInterestSet(_)
        | FeedSessionResourceCommand::ReplayFromStore(_)
        | FeedSessionResourceCommand::AttachProjection(_)
        | FeedSessionResourceCommand::DetachProjection(_) => None,
    }
}

fn active_child_from_demand(demand: &InterestDemand) -> ActiveChild {
    ActiveChild {
        child: DependentInterestChild::tailing(demand.shape.clone(), interest_scope(&demand.scope)),
        scope: demand.scope.key_part(),
        provenance: demand.provenance.key_part(),
    }
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
    interest_key: &ResourceKey,
    active: &ActiveChild,
) -> FeedSessionDiagnosticInterest {
    FeedSessionDiagnosticInterest::new(
        interest_key.as_str(),
        active.scope.clone(),
        format!(
            "lifecycle={}:shape={}",
            lifecycle_part(&active.child.interest.lifecycle),
            digest(("interest-shape", &active.child.interest.shape))
        ),
        Some(active.child.interest.id.0.to_string()),
        active.provenance,
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
