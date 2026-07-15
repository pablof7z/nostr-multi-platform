use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(test)]
use crate::trellis_adapter_trace::{
    output_frame_kinds, resource_traces, FeedSessionOutputFrameKind, FeedSessionResourceTrace,
};
use nmp_core::actor::ActorCommand;
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSender, DependentInterestDelta};
use nmp_feed::{FeedShape, ProjectionKey, TeardownAction};
use trellis_core::{
    DependencyList, Graph, InputNode, MaterializedOutput, OutputFrame, OutputFrameKind,
    ResourceKey, ResourcePlan, ScopeId,
};

use crate::diagnostics::{
    FeedSessionDiagnosticBatch, FeedSessionDiagnosticContext, FeedSessionDiagnosticReasonCode,
    FeedSessionDiagnosticsHandle,
};
use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_adapter_command::FeedSessionTrellisCommand;
use crate::trellis_adapter_delivery::FeedSessionDeliveryQueue;
use crate::trellis_adapter_delta::FeedSessionResourceLedger;
use crate::trellis_adapter_diagnostics::{diagnostic_batch, diagnostic_context};
use crate::trellis_owner_cell::TrellisGraphCell;
use crate::trellis_resources::{
    FeedSessionResourceCommand, FeedSessionResourceKey, FeedSessionScopeKey, ProjectionAttachment,
};
use crate::FeedOpenError;
#[cfg(test)]
use trellis_core::ResourceCommand;

type DemandMap = BTreeMap<FeedSessionResourceKey, AcquisitionInterest>;
type FeedSessionOutput = ProjectionAttachment;

/// Private NMP/Trellis adapter for feed-session resource reconciliation.
///
/// Callers pass NMP-owned acquisition interests and Trellis emits precise
/// dependent-interest deltas. The adapter also owns the session's output
/// lifecycle ledger: Trellis emits output baseline, rebaseline, and clear
/// frames internally; NMP still owns typed projection encoding and registry
/// mutation. Trellis graph/scope/node/output identities stay inside this
/// module.
#[derive(Clone)]
pub(super) struct FeedSessionTrellisAdapter {
    inner: Arc<TrellisGraphCell<FeedSessionTrellisInner>>,
    sender: CommandSender,
    owner: SubOwnerKey,
    diagnostics: FeedSessionDiagnosticsHandle,
    delivery: Arc<Mutex<FeedSessionDeliveryQueue>>,
}

struct FeedSessionTrellisInner {
    graph: Graph<FeedSessionResourceCommand>,
    scope: ScopeId,
    demand_input: InputNode<DemandMap>,
    output: MaterializedOutput<FeedSessionOutput>,
    fixed: Vec<AcquisitionInterest>,
    resource_ledger: FeedSessionResourceLedger,
    diagnostic_context: FeedSessionDiagnosticContext,
    closed: bool,
    #[cfg(test)]
    output_frames: Vec<FeedSessionOutputFrameKind>,
    #[cfg(test)]
    resource_traces: Vec<FeedSessionResourceTrace>,
}

struct FeedSessionCloseOutcome {
    output_cleared: bool,
    interest_delta: Option<DependentInterestDelta>,
    diagnostics: Option<FeedSessionDiagnosticBatch>,
}

struct FeedSessionTrellisSyncOutcome {
    interest_delta: Option<DependentInterestDelta>,
    diagnostics: Option<FeedSessionDiagnosticBatch>,
}

impl FeedSessionTrellisAdapter {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "adapter tests use the default no-diagnostics constructor; session builders pass the host diagnostics handle"
        )
    )]
    pub(super) fn new(
        projection_key: &str,
        shape: FeedShape,
        fixed: Vec<AcquisitionInterest>,
        sender: CommandSender,
    ) -> Result<Self, FeedOpenError> {
        Self::new_with_diagnostics(
            projection_key,
            shape,
            fixed,
            sender,
            FeedSessionDiagnosticsHandle::disabled(),
        )
    }

    pub(super) fn new_with_diagnostics(
        projection_key: &str,
        shape: FeedShape,
        fixed: Vec<AcquisitionInterest>,
        sender: CommandSender,
        diagnostics: FeedSessionDiagnosticsHandle,
    ) -> Result<Self, FeedOpenError> {
        let projection = ProjectionKey::app_owned(projection_key)
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let scope_key = FeedSessionScopeKey::projection(&projection);
        let owner = session_acquisition_owner(projection_key);
        let diagnostic_context = diagnostic_context(&projection, &shape, &scope_key, owner);
        let output_attachment = ProjectionAttachment::new(projection.clone(), shape);

        let mut graph = Graph::<FeedSessionResourceCommand>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .map_err(|_| FeedOpenError::RegistryUnavailable)?;
        let scope = tx
            .create_scope(scope_key.as_str())
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
            inner: Arc::new(TrellisGraphCell::new(FeedSessionTrellisInner {
                graph,
                scope,
                demand_input,
                output,
                fixed,
                resource_ledger: FeedSessionResourceLedger::default(),
                diagnostic_context,
                closed: false,
                #[cfg(test)]
                output_frames,
                #[cfg(test)]
                resource_traces: Vec::new(),
            })),
            sender,
            owner,
            diagnostics,
            delivery: Arc::new(Mutex::new(FeedSessionDeliveryQueue::default())),
        })
    }

    pub(super) fn sync(&self, extra: &ExtraAcquisition, reason: &'static str) -> bool {
        self.sync_with_diagnostic_reason(
            extra,
            reason,
            FeedSessionDiagnosticReasonCode::AcquisitionSync,
        )
    }

    pub(super) fn sync_with_diagnostic_reason(
        &self,
        extra: &ExtraAcquisition,
        reason: &'static str,
        diagnostic_reason: FeedSessionDiagnosticReasonCode,
    ) -> bool {
        let mut delivered = self.flush_pending_delivery();
        let diagnostics_enabled = self.diagnostics.is_enabled();
        let Some(outcome) = self.inner.with_mut("sync", |inner| {
            inner.sync(extra, diagnostics_enabled, diagnostic_reason, reason)
        }) else {
            return delivered;
        };
        self.record_diagnostics(outcome.diagnostics);
        if let Some(delta) = outcome.interest_delta {
            self.queue_delta(delta, reason);
            delivered |= self.flush_pending_delivery();
        }
        delivered
    }

    #[cfg(test)]
    pub(super) fn sync_source_effect_for_test(
        &self,
        extra: &ExtraAcquisition,
        reason: &'static str,
    ) -> bool {
        self.sync_with_diagnostic_reason(
            extra,
            reason,
            FeedSessionDiagnosticReasonCode::SourceEffect,
        )
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

    pub(super) fn schedule_source_effect(
        &self,
        extra: ExtraAcquisition,
        reason: &'static str,
        rebaseline: bool,
    ) {
        let _ = self.sender.send(ActorCommand::Protocol(Box::new(
            FeedSessionTrellisCommand::source_effect(self.clone(), extra, reason, rebaseline),
        )));
    }

    pub(super) fn close_action(&self, remove_projection: TeardownAction) -> TeardownAction {
        let adapter = self.clone();
        Box::new(move || {
            adapter.flush_pending_delivery();
            let Some(outcome) = adapter.inner.with_mut("close", |inner| {
                inner.close_scope(adapter.diagnostics.is_enabled())
            }) else {
                return;
            };
            adapter.record_diagnostics(outcome.diagnostics);
            if let Some(delta) = outcome.interest_delta {
                adapter.queue_delta(delta, "feed-session-acquisition-close");
            }
            if outcome.output_cleared {
                adapter.queue_output_clear(remove_projection);
            }
            adapter.flush_pending_delivery();
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

    fn queue_delta(&self, delta: DependentInterestDelta, reason: &'static str) {
        self.delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_delta(delta, reason);
    }

    fn queue_output_clear(&self, action: TeardownAction) {
        self.delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_output_clear(action);
    }

    fn flush_pending_delivery(&self) -> bool {
        let flush = self
            .delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .flush(&self.sender, self.owner);
        if let Some(output_clear) = flush.output_clear {
            output_clear();
        }
        flush.delivered_delta
    }

    fn record_diagnostics(&self, diagnostics: Option<FeedSessionDiagnosticBatch>) {
        let Some(batch) = diagnostics else {
            return;
        };
        self.diagnostics.record(batch);
    }
}

impl FeedSessionTrellisInner {
    fn sync(
        &mut self,
        extra: &ExtraAcquisition,
        diagnostics_enabled: bool,
        diagnostic_reason: FeedSessionDiagnosticReasonCode,
        reason_label: &'static str,
    ) -> Option<FeedSessionTrellisSyncOutcome> {
        if self.closed {
            return None;
        }
        let demand = demand_map(&self.fixed, extra);
        let mut tx = self.graph.begin_transaction().ok()?;
        tx.set_input(self.demand_input, demand).ok()?;
        let result = tx.commit().ok()?;
        drop(tx);
        #[cfg(test)]
        self.record_output_frames(&result.output_frames);
        #[cfg(test)]
        self.record_resource_traces(result.resource_plan.commands());
        let output = self
            .resource_ledger
            .delta_with_diagnostics(result.resource_plan.commands(), diagnostics_enabled);
        let diagnostics = diagnostic_batch(
            &self.diagnostic_context,
            output.diagnostics,
            &result,
            diagnostic_reason,
            reason_label,
        );
        let interest_delta = (!output.delta.is_empty()).then_some(output.delta);
        if interest_delta.is_none() && diagnostics.is_none() {
            return None;
        }
        Some(FeedSessionTrellisSyncOutcome {
            interest_delta,
            diagnostics,
        })
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

    fn close_scope(&mut self, diagnostics_enabled: bool) -> Option<FeedSessionCloseOutcome> {
        if self.closed {
            return None;
        }
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
        self.closed = true;
        drop(tx);
        #[cfg(test)]
        self.record_output_frames(&result.output_frames);
        #[cfg(test)]
        self.record_resource_traces(result.resource_plan.commands());
        let output = self
            .resource_ledger
            .delta_with_diagnostics(result.resource_plan.commands(), diagnostics_enabled);
        let diagnostics = diagnostic_batch(
            &self.diagnostic_context,
            output.diagnostics,
            &result,
            FeedSessionDiagnosticReasonCode::AcquisitionClose,
            "feed-session-acquisition-close",
        );
        let interest_delta = (!output.delta.is_empty()).then_some(output.delta);
        Some(FeedSessionCloseOutcome {
            output_cleared: output_cleared(&result.output_frames),
            interest_delta,
            diagnostics,
        })
    }

    #[cfg(test)]
    fn record_output_frames(&mut self, frames: &[OutputFrame]) {
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
            output_cleared: false,
            interest_delta: None,
            diagnostics: None,
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

fn trellis_key(key: &FeedSessionResourceKey) -> ResourceKey {
    ResourceKey::new(key.as_str().to_string())
}

fn output_cleared(frames: &[OutputFrame]) -> bool {
    frames
        .iter()
        .any(|frame| matches!(frame.kind, OutputFrameKind::Clear(_)))
}
