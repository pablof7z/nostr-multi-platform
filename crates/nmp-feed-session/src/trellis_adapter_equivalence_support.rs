use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

use nmp_core::actor::{ActorCommand, ActorMail, InterestsCommand, LifecycleCommand};
use nmp_core::{CommandSender, DependentInterestChild, DependentInterestDeltaCommand};
use nmp_planner::InterestShape;

use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_adapter::FeedSessionTrellisAdapter;
use crate::trellis_adapter_trace::{FeedSessionResourceTrace, FeedSessionResourceTraceKind};
use crate::trellis_resources::FeedSessionRouteProvenance;

#[derive(Default)]
pub(super) struct OldReplacementPath {
    current: BTreeSet<String>,
    authority: BTreeSet<String>,
}

struct StepResult {
    old_recompute: Option<BTreeSet<String>>,
    resource_traces: BTreeSet<FeedSessionResourceTrace>,
}

fn author_shape(author: &str) -> InterestShape {
    InterestShape::timeline_for(
        BTreeSet::from([author.to_string()]),
        BTreeSet::from([1_u32]),
    )
}

fn interest(author: &str) -> AcquisitionInterest {
    AcquisitionInterest::active_account_with_provenance(
        author_shape(author),
        FeedSessionRouteProvenance::ActiveFollowTimeline,
    )
}

fn resource_key(author: &str) -> String {
    interest(author).resource_key().as_str().to_string()
}

pub(super) fn extra_from_authors(authors: &[&str]) -> ExtraAcquisition {
    let interests: Vec<AcquisitionInterest> =
        authors.iter().map(|author| interest(author)).collect();
    Arc::new(move || interests.clone())
}

fn full_recompute(authors: &[&str]) -> BTreeSet<String> {
    authors.iter().map(|author| (*author).to_string()).collect()
}

impl OldReplacementPath {
    fn apply(&mut self, authors: &[&str]) -> Option<BTreeSet<String>> {
        let recomputed = full_recompute(authors);
        if recomputed == self.current {
            None
        } else {
            self.current = recomputed.clone();
            Some(recomputed)
        }
    }
}

pub(super) fn command_receiver() -> (CommandSender, mpsc::Receiver<ActorMail>) {
    let (tx, rx) = mpsc::channel();
    (CommandSender::new(tx), rx)
}

pub(super) fn drain_delta_commands(
    rx: &mpsc::Receiver<ActorMail>,
) -> Vec<(Vec<DependentInterestDeltaCommand>, String)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|mail| match mail {
            ActorMail::Command(ActorCommand::Interests(
                InterestsCommand::ApplyDependentInterestDelta { delta, reason, .. },
            )) => (delta.commands, reason),
            _ => panic!("unexpected actor mail"),
        })
        .collect()
}

pub(super) fn drain_mark_changed(rx: &mpsc::Receiver<ActorMail>) -> usize {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|mail| match mail {
            ActorMail::Command(ActorCommand::Lifecycle(LifecycleCommand::MarkChangedSinceEmit)) => {
                1
            }
            _ => panic!("unexpected actor mail"),
        })
        .sum()
}

fn authors(children: &[DependentInterestChild]) -> BTreeSet<String> {
    children
        .iter()
        .flat_map(|child| child.interest.shape.authors.iter().cloned())
        .collect()
}

pub(super) fn delta_close_authors(commands: &[DependentInterestDeltaCommand]) -> BTreeSet<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            DependentInterestDeltaCommand::Close(child) => Some(child),
            DependentInterestDeltaCommand::Open(_)
            | DependentInterestDeltaCommand::Replace(_)
            | DependentInterestDeltaCommand::Refresh(_) => None,
        })
        .flat_map(|child| authors(std::slice::from_ref(child)))
        .collect()
}

fn apply_delta(current: &mut BTreeSet<String>, commands: &[DependentInterestDeltaCommand]) {
    for command in commands {
        match command {
            DependentInterestDeltaCommand::Open(child)
            | DependentInterestDeltaCommand::Replace(child)
            | DependentInterestDeltaCommand::Refresh(child) => {
                current.extend(authors(std::slice::from_ref(child)));
            }
            DependentInterestDeltaCommand::Close(child) => {
                for author in authors(std::slice::from_ref(child)) {
                    current.remove(&author);
                }
            }
        }
    }
}

fn sync_step(
    adapter: &FeedSessionTrellisAdapter,
    rx: &mpsc::Receiver<ActorMail>,
    old: &mut OldReplacementPath,
    authors: &[&str],
    reason: &'static str,
) -> StepResult {
    let trace_start = adapter.resource_traces_for_test().len();
    let old_result = old.apply(authors);
    let changed = adapter.sync(&extra_from_authors(authors), reason);
    assert_eq!(
        changed,
        old_result.is_some(),
        "Trellis adapter and old recompute path disagree for {reason}"
    );

    let deltas = drain_delta_commands(rx);
    match old_result {
        Some(ref expected) => {
            assert_eq!(deltas.len(), 1, "{reason} must emit one delta");
            apply_delta(&mut old.authority, &deltas[0].0);
            assert_eq!(
                old.authority, *expected,
                "{reason} delta-applied authority must equal full recompute"
            );
            assert_eq!(deltas[0].1, reason);
        }
        None => assert!(
            deltas.is_empty(),
            "{reason} must not emit shell-visible churn"
        ),
    }

    let resource_traces = adapter
        .resource_traces_for_test()
        .into_iter()
        .skip(trace_start)
        .collect();
    StepResult {
        old_recompute: old_result,
        resource_traces,
    }
}

pub(super) fn assert_changed_step(
    adapter: &FeedSessionTrellisAdapter,
    rx: &mpsc::Receiver<ActorMail>,
    old: &mut OldReplacementPath,
    authors: &[&str],
    reason: &'static str,
    expected_traces: BTreeSet<FeedSessionResourceTrace>,
) {
    let step = sync_step(adapter, rx, old, authors, reason);
    assert_eq!(
        step.old_recompute,
        Some(full_recompute(authors)),
        "{reason} old recompute must equal full recompute"
    );
    assert_eq!(step.resource_traces, expected_traces, "{reason} traces");
}

pub(super) fn assert_unchanged_step(
    adapter: &FeedSessionTrellisAdapter,
    rx: &mpsc::Receiver<ActorMail>,
    old: &mut OldReplacementPath,
    authors: &[&str],
    reason: &'static str,
) {
    let step = sync_step(adapter, rx, old, authors, reason);
    assert!(
        step.old_recompute.is_none(),
        "{reason} must not emit a delta"
    );
    assert!(
        step.resource_traces.is_empty(),
        "{reason} must not emit resource churn"
    );
}

pub(super) fn expected_traces(
    kind: FeedSessionResourceTraceKind,
    authors: &[&str],
) -> BTreeSet<FeedSessionResourceTrace> {
    authors
        .iter()
        .map(|author| FeedSessionResourceTrace {
            kind,
            key: resource_key(author),
        })
        .collect()
}

pub(super) fn trace_delta(
    adapter: &FeedSessionTrellisAdapter,
    start: usize,
) -> BTreeSet<FeedSessionResourceTrace> {
    adapter
        .resource_traces_for_test()
        .into_iter()
        .skip(start)
        .collect()
}

pub(super) fn remove_projection_action(count: Arc<AtomicUsize>) -> nmp_feed::TeardownAction {
    Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })
}
