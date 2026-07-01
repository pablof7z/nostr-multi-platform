use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSender, DependentInterestChild};
use nmp_feed::TeardownAction;
use trellis_core::{DependencyList, Graph, InputNode, ResourceKey, ResourcePlan, ScopeId};

use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_resources::{
    FeedSessionResourceCommand, FeedSessionResourceKey, FeedSessionScopeKey,
};
use crate::FeedOpenError;

type DemandMap = BTreeMap<FeedSessionResourceKey, AcquisitionInterest>;

/// Private NMP/Trellis adapter for feed-session resource reconciliation.
///
/// Callers pass NMP-owned acquisition interests and receive the same actor
/// replacement command the session engine already uses. Trellis graph/scope/node
/// identities stay inside this module.
#[derive(Clone)]
pub(super) struct FeedSessionTrellisAdapter {
    inner: Arc<Mutex<FeedSessionTrellisInner>>,
    sender: CommandSender,
    owner: SubOwnerKey,
}

struct FeedSessionTrellisInner {
    graph: Graph<FeedSessionResourceCommand>,
    scope: ScopeId,
    demand_input: InputNode<DemandMap>,
    fixed: Vec<AcquisitionInterest>,
    closed: bool,
}

impl FeedSessionTrellisAdapter {
    pub(super) fn new(
        projection_key: &str,
        fixed: Vec<AcquisitionInterest>,
        sender: CommandSender,
    ) -> Result<Self, FeedOpenError> {
        let mut graph = Graph::<FeedSessionResourceCommand>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let scope = tx
            .create_scope(
                FeedSessionScopeKey::projection(
                    &nmp_feed::ProjectionKey::app_owned(projection_key)
                        .map_err(|_| FeedOpenError::RegistryUnavailable)?,
                )
                .as_str(),
            )
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let demand_input = tx
            .input::<DemandMap>("feed-session-acquisition-demand")
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.set_input(demand_input, DemandMap::new())
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.attach_node_to_scope(demand_input, scope)
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let demand = tx
            .map_collection(
                "feed-session-acquisition-demand-map",
                DependencyList::new([demand_input.id()])
                    .map_err(|_| FeedOpenError::RegistryUnavailable)?,
                move |ctx| Ok(ctx.input(demand_input)?.clone()),
            )
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.map_resource_planner(demand, scope, move |ctx| {
            let mut plan = ResourcePlan::new();
            for added in &ctx.diff().added {
                let (key, interest) = &added.value;
                plan.open(
                    trellis_key(key),
                    ctx.scope(),
                    FeedSessionResourceCommand::OpenInterest(interest.demand()),
                );
            }
            for updated in &ctx.diff().updated {
                plan.replace(
                    trellis_key(&updated.key),
                    ctx.scope(),
                    FeedSessionResourceCommand::OpenInterest(updated.current.demand()),
                );
            }
            for removed in &ctx.diff().removed {
                let (key, interest) = &removed.value;
                plan.close(trellis_key(key), ctx.scope());
                let _ = interest;
            }
            Ok(plan)
        })
        .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.commit()
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        drop(tx);

        Ok(Self {
            inner: Arc::new(Mutex::new(FeedSessionTrellisInner {
                graph,
                scope,
                demand_input,
                fixed,
                closed: false,
            })),
            sender,
            owner: session_acquisition_owner(projection_key),
        })
    }

    pub(super) fn sync(&self, extra: &ExtraAcquisition, reason: &'static str) -> bool {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return false,
        };
        let Some(children) = inner.sync(extra) else {
            return false;
        };
        drop(inner);
        self.replace_children(children, reason);
        true
    }

    pub(super) fn close_action(&self) -> TeardownAction {
        let adapter = self.clone();
        Box::new(move || {
            let mut inner = match adapter.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if !inner.close_scope() {
                return;
            }
            drop(inner);
            adapter.replace_children(Vec::new(), "feed-session-acquisition-close");
        })
    }

    fn replace_children(&self, children: Vec<DependentInterestChild>, reason: &'static str) {
        let _ = self.sender.send(ActorCommand::Interests(
            InterestsCommand::ReplaceDependentInterestSet {
                owner: self.owner,
                children,
                reason: reason.to_string(),
            },
        ));
    }
}

impl FeedSessionTrellisInner {
    fn sync(&mut self, extra: &ExtraAcquisition) -> Option<Vec<DependentInterestChild>> {
        if self.closed {
            return None;
        }
        let demand = demand_map(&self.fixed, extra);
        let mut tx = self.graph.begin_transaction().ok()?;
        tx.set_input(self.demand_input, demand.clone()).ok()?;
        let result = tx.commit().ok()?;
        drop(tx);
        (!result.resource_plan.commands().is_empty()).then(|| children_from_demand(&demand))
    }

    fn close_scope(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.closed = true;
        let mut tx = match self.graph.begin_transaction() {
            Ok(tx) => tx,
            Err(_) => return true,
        };
        let _ = tx.close_scope(self.scope);
        let _ = tx.commit();
        true
    }
}

fn session_acquisition_owner(key: &str) -> SubOwnerKey {
    SubOwnerKey::new(("feed-session-acquisition", key))
}

fn demand_map(fixed: &[AcquisitionInterest], extra: &ExtraAcquisition) -> DemandMap {
    fixed
        .iter()
        .cloned()
        .chain(extra())
        .map(|interest| (interest.resource_key(), interest))
        .collect()
}

fn children_from_demand(demand: &DemandMap) -> Vec<DependentInterestChild> {
    demand.values().map(AcquisitionInterest::to_child).collect()
}

fn trellis_key(key: &FeedSessionResourceKey) -> ResourceKey {
    ResourceKey::new(key.as_str().to_string())
}
