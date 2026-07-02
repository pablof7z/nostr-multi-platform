use std::cell::{Cell, UnsafeCell};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::{self, ThreadId};

use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSender, DependentInterestChild};
use nmp_feed::{FeedShape, ProjectionKey, TeardownAction};
#[cfg(test)]
use trellis_core::ResourceCommand;
use trellis_core::{
    DependencyList, Graph, InputNode, MaterializedOutput, OutputFrame, OutputFrameKind,
    ResourceKey, ResourcePlan, ScopeId,
};

use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_resources::{
    FeedSessionResourceCommand, FeedSessionResourceKey, FeedSessionScopeKey, ProjectionAttachment,
};
use crate::FeedOpenError;

type DemandMap = BTreeMap<FeedSessionResourceKey, AcquisitionInterest>;
type FeedSessionOutput = ProjectionAttachment;

/// Private NMP/Trellis adapter for feed-session resource reconciliation.
///
/// Callers pass NMP-owned acquisition interests and receive the same actor
/// replacement command the session engine already uses. The adapter also owns
/// the session's output lifecycle ledger: Trellis emits output baseline,
/// rebaseline, and clear frames internally; NMP still owns typed projection
/// encoding and registry mutation. Trellis graph/scope/node/output identities
/// stay inside this module.
#[derive(Clone)]
pub(super) struct FeedSessionTrellisAdapter {
    inner: Arc<ActorThreadCell<FeedSessionTrellisInner>>,
    sender: CommandSender,
    owner: SubOwnerKey,
}

struct FeedSessionTrellisInner {
    graph: Graph<FeedSessionResourceCommand, FeedSessionOutput>,
    scope: ScopeId,
    demand_input: InputNode<DemandMap>,
    output: MaterializedOutput<FeedSessionOutput>,
    fixed: Vec<AcquisitionInterest>,
    closed: bool,
    #[cfg(test)]
    output_frames: Vec<FeedSessionOutputFrameKind>,
    #[cfg(test)]
    resource_traces: Vec<FeedSessionResourceTrace>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FeedSessionOutputFrameKind {
    Baseline,
    Delta,
    Rebaseline,
    Clear,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FeedSessionResourceTraceKind {
    Open,
    Replace,
    Refresh,
    Close,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct FeedSessionResourceTrace {
    pub(super) kind: FeedSessionResourceTraceKind,
    pub(super) key: String,
}

struct FeedSessionCloseOutcome {
    output_cleared: bool,
}

struct ActorThreadCell<T> {
    owner: ThreadId,
    borrowed: Cell<bool>,
    value: UnsafeCell<T>,
}

// Safety: callers can move/share the handle to satisfy callback traits, but
// `with_ref`/`with_mut` only expose the value on the creating actor thread.
unsafe impl<T: Send> Send for ActorThreadCell<T> {}
unsafe impl<T: Send> Sync for ActorThreadCell<T> {}

impl<T> ActorThreadCell<T> {
    fn new(value: T) -> Self {
        Self {
            owner: thread::current().id(),
            borrowed: Cell::new(false),
            value: UnsafeCell::new(value),
        }
    }

    fn with_mut<R>(&self, operation: &'static str, f: impl FnOnce(&mut T) -> R) -> R {
        self.assert_owner(operation);
        self.with_exclusive_borrow(operation, || f(unsafe { &mut *self.value.get() }))
    }

    #[cfg(test)]
    fn with_ref<R>(&self, operation: &'static str, f: impl FnOnce(&T) -> R) -> R {
        self.assert_owner(operation);
        self.with_exclusive_borrow(operation, || f(unsafe { &*self.value.get() }))
    }

    fn with_exclusive_borrow<R>(&self, operation: &'static str, f: impl FnOnce() -> R) -> R {
        assert!(
            !self.borrowed.replace(true),
            "FeedSessionTrellisAdapter {operation} re-entered while the Trellis graph is borrowed"
        );
        let _guard = ActorThreadBorrowGuard {
            borrowed: &self.borrowed,
        };
        f()
    }

    fn assert_owner(&self, operation: &'static str) {
        assert_eq!(
            thread::current().id(),
            self.owner,
            "FeedSessionTrellisAdapter {operation} called outside its owner actor thread"
        );
    }
}

struct ActorThreadBorrowGuard<'a> {
    borrowed: &'a Cell<bool>,
}

impl Drop for ActorThreadBorrowGuard<'_> {
    fn drop(&mut self) {
        self.borrowed.set(false);
    }
}

impl FeedSessionTrellisAdapter {
    pub(super) fn new(
        projection_key: &str,
        shape: FeedShape,
        fixed: Vec<AcquisitionInterest>,
        sender: CommandSender,
    ) -> Result<Self, FeedOpenError> {
        let projection = ProjectionKey::app_owned(projection_key)
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let output_attachment = ProjectionAttachment::new(projection.clone(), shape);

        let mut graph =
            Graph::<FeedSessionResourceCommand, FeedSessionOutput>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let scope = tx
            .create_scope(FeedSessionScopeKey::projection(&projection).as_str())
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

        let output_input = tx
            .input::<FeedSessionOutput>("feed-session-output-attachment")
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.set_input(output_input, output_attachment)
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        tx.attach_node_to_scope(output_input, scope)
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let output = tx
            .materialized_output(
                "feed-session-output",
                scope,
                DependencyList::new([output_input.id()])
                    .map_err(|_| FeedOpenError::RegistryUnavailable)?,
                move |ctx| Ok(ctx.input(output_input)?.clone()),
            )
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let _result = tx
            .commit()
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        drop(tx);
        #[cfg(test)]
        let output_frames = output_frame_kinds(&_result.output_frames);

        Ok(Self {
            inner: Arc::new(ActorThreadCell::new(FeedSessionTrellisInner {
                graph,
                scope,
                demand_input,
                output,
                fixed,
                closed: false,
                #[cfg(test)]
                output_frames,
                #[cfg(test)]
                resource_traces: Vec::new(),
            })),
            sender,
            owner: session_acquisition_owner(projection_key),
        })
    }

    pub(super) fn sync(&self, extra: &ExtraAcquisition, reason: &'static str) -> bool {
        let Some(children) = self.inner.with_mut("sync", |inner| inner.sync(extra)) else {
            return false;
        };
        self.replace_children(children, reason);
        true
    }

    pub(super) fn rebaseline_output_if_changed(&self, changed: bool) -> bool {
        if !changed {
            return false;
        }
        if !self
            .inner
            .with_mut("rebaseline", FeedSessionTrellisInner::rebaseline_output)
        {
            return false;
        }
        self.sender.mark_changed_since_emit();
        true
    }

    pub(super) fn close_action(&self, remove_projection: TeardownAction) -> TeardownAction {
        let adapter = self.clone();
        Box::new(move || {
            let Some(outcome) = adapter
                .inner
                .with_mut("close", FeedSessionTrellisInner::close_scope)
            else {
                return;
            };
            if outcome.output_cleared {
                remove_projection();
            }
            adapter.replace_children(Vec::new(), "feed-session-acquisition-close");
        })
    }

    #[cfg(test)]
    pub(super) fn output_frame_kinds_for_test(&self) -> Vec<FeedSessionOutputFrameKind> {
        self.inner.with_ref("output-frame-test-read", |inner| {
            inner.output_frames.clone()
        })
    }

    #[cfg(test)]
    pub(super) fn resource_traces_for_test(&self) -> Vec<FeedSessionResourceTrace> {
        self.inner.with_ref("resource-trace-test-read", |inner| {
            inner.resource_traces.clone()
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
        #[cfg(test)]
        self.record_output_frames(&result.output_frames);
        #[cfg(test)]
        self.record_resource_traces(result.resource_plan.commands());
        (!result.resource_plan.commands().is_empty()).then(|| children_from_demand(&demand))
    }

    fn rebaseline_output(&mut self) -> bool {
        if self.closed {
            return false;
        }
        let mut tx = match self.graph.begin_transaction() {
            Ok(tx) => tx,
            Err(_) => return false,
        };
        if tx.rebaseline_output(self.output.clone()).is_err() {
            return false;
        }
        let result = match tx.commit() {
            Ok(result) => result,
            Err(_) => return false,
        };
        drop(tx);
        #[cfg(test)]
        self.record_output_frames(&result.output_frames);
        result
            .output_frames
            .iter()
            .any(|frame| matches!(frame.kind, OutputFrameKind::Rebaseline(_, _)))
    }

    fn close_scope(&mut self) -> Option<FeedSessionCloseOutcome> {
        if self.closed {
            return None;
        }
        self.closed = true;
        let mut tx = match self.graph.begin_transaction() {
            Ok(tx) => tx,
            Err(_) => return Some(FeedSessionCloseOutcome::best_effort()),
        };
        if tx.close_scope(self.scope).is_err() {
            return Some(FeedSessionCloseOutcome::best_effort());
        }
        let result = match tx.commit() {
            Ok(result) => result,
            Err(_) => return Some(FeedSessionCloseOutcome::best_effort()),
        };
        drop(tx);
        #[cfg(test)]
        self.record_output_frames(&result.output_frames);
        #[cfg(test)]
        self.record_resource_traces(result.resource_plan.commands());
        Some(FeedSessionCloseOutcome {
            output_cleared: output_cleared(&result.output_frames),
        })
    }

    #[cfg(test)]
    fn record_output_frames(&mut self, frames: &[OutputFrame<FeedSessionOutput>]) {
        self.output_frames.extend(output_frame_kinds(frames));
    }

    #[cfg(test)]
    fn record_resource_traces(&mut self, commands: &[ResourceCommand<FeedSessionResourceCommand>]) {
        self.resource_traces.extend(resource_traces(commands));
    }
}

impl FeedSessionCloseOutcome {
    fn best_effort() -> Self {
        Self {
            output_cleared: true,
        }
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

fn output_cleared(frames: &[OutputFrame<FeedSessionOutput>]) -> bool {
    frames
        .iter()
        .any(|frame| matches!(frame.kind, OutputFrameKind::Clear(_)))
}

#[cfg(test)]
fn output_frame_kinds(
    frames: &[OutputFrame<FeedSessionOutput>],
) -> Vec<FeedSessionOutputFrameKind> {
    frames
        .iter()
        .map(|frame| match frame.kind {
            OutputFrameKind::Baseline(_) => FeedSessionOutputFrameKind::Baseline,
            OutputFrameKind::Delta(_) => FeedSessionOutputFrameKind::Delta,
            OutputFrameKind::Rebaseline(_, _) => FeedSessionOutputFrameKind::Rebaseline,
            OutputFrameKind::Clear(_) => FeedSessionOutputFrameKind::Clear,
        })
        .collect()
}

#[cfg(test)]
fn resource_traces(
    commands: &[ResourceCommand<FeedSessionResourceCommand>],
) -> Vec<FeedSessionResourceTrace> {
    commands
        .iter()
        .map(|command| {
            let kind = match command {
                ResourceCommand::Open { .. } => FeedSessionResourceTraceKind::Open,
                ResourceCommand::Replace { .. } => FeedSessionResourceTraceKind::Replace,
                ResourceCommand::Refresh { .. } => FeedSessionResourceTraceKind::Refresh,
                ResourceCommand::Close { .. } => FeedSessionResourceTraceKind::Close,
            };
            FeedSessionResourceTrace {
                kind,
                key: command.key().as_str().to_string(),
            }
        })
        .collect()
}
