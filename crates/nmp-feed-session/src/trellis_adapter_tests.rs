use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;

use nmp_core::actor::{ActorCommand, ActorMail, InterestsCommand, LifecycleCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSender, DependentInterestChild, DependentInterestDeltaCommand};
use nmp_feed::FeedShape;
use nmp_planner::InterestShape;

use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_adapter::FeedSessionTrellisAdapter;
use crate::trellis_adapter_trace::FeedSessionOutputFrameKind;
use crate::trellis_resources::FeedSessionRouteProvenance;
use crate::{
    FeedSessionDiagnosticBatch, FeedSessionDiagnosticEventKind, FeedSessionDiagnosticReasonCode,
    FeedSessionDiagnosticsHandle, FeedSessionDiagnosticsSink,
};

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

fn extra(values: Vec<AcquisitionInterest>) -> ExtraAcquisition {
    Arc::new(move || values.clone())
}

fn command_receiver() -> (CommandSender, mpsc::Receiver<ActorMail>) {
    let (tx, rx) = mpsc::channel();
    (CommandSender::new(tx), rx)
}

#[derive(Default)]
struct RecordingDiagnostics {
    batches: Mutex<Vec<FeedSessionDiagnosticBatch>>,
}

impl RecordingDiagnostics {
    fn batches(&self) -> Vec<FeedSessionDiagnosticBatch> {
        self.batches.lock().unwrap().clone()
    }
}

impl FeedSessionDiagnosticsSink for RecordingDiagnostics {
    fn record(&self, batch: FeedSessionDiagnosticBatch) {
        self.batches.lock().unwrap().push(batch);
    }
}

fn drain_deltas(
    rx: &mpsc::Receiver<ActorMail>,
) -> Vec<(SubOwnerKey, Vec<DependentInterestDeltaCommand>, String)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|mail| match mail {
            ActorMail::Command(ActorCommand::Interests(
                InterestsCommand::ApplyDependentInterestDelta {
                    owner,
                    delta,
                    reason,
                },
            )) => (owner, delta.commands, reason),
            _ => panic!("unexpected actor mail"),
        })
        .collect()
}

fn drain_mark_changed(rx: &mpsc::Receiver<ActorMail>) -> usize {
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

fn open_authors(commands: &[DependentInterestDeltaCommand]) -> BTreeSet<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            DependentInterestDeltaCommand::Open(child)
            | DependentInterestDeltaCommand::Replace(child)
            | DependentInterestDeltaCommand::Refresh(child) => Some(child),
            DependentInterestDeltaCommand::Close(_) => None,
        })
        .flat_map(|child| authors(std::slice::from_ref(child)))
        .collect()
}

fn close_authors(commands: &[DependentInterestDeltaCommand]) -> BTreeSet<String> {
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

#[test]
fn adapter_emits_live_diagnostics_when_sink_is_enabled() {
    let (sender, _rx) = command_receiver();
    let diagnostics = Arc::new(RecordingDiagnostics::default());
    let adapter = FeedSessionTrellisAdapter::new_with_diagnostics(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender,
        FeedSessionDiagnosticsHandle::new(diagnostics.clone()),
    )
    .unwrap();

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));

    let batches = diagnostics.batches();
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(batch.projection_key, "app.feed.synthetic");
    assert_eq!(batch.view_label, "flat");
    assert_eq!(
        batch.reason.code,
        FeedSessionDiagnosticReasonCode::AcquisitionSync
    );
    assert_eq!(batch.reason.label, "source-changed");
    assert!(batch.transaction.transaction > 0);
    assert_eq!(batch.receipts.len(), 1);
    let receipt = &batch.receipts[0];
    assert_eq!(receipt.event, FeedSessionDiagnosticEventKind::Open);
    assert_eq!(receipt.owner_counts.before, 0);
    assert_eq!(receipt.owner_counts.after, 1);
    let interest = receipt.interest.as_ref().unwrap();
    assert_eq!(interest.scope, "active-account");
    assert_eq!(interest.provenance, "active-follow-timeline");
    assert!(interest.shape.starts_with("lifecycle=tailing:shape="));
    assert!(
        interest
            .planner_interest_id_hint
            .as_deref()
            .is_some_and(|interest_id| interest_id.parse::<u64>().is_ok()),
        "diagnostic interest should expose a planner interest id hint"
    );
    assert!(
        !interest.shape.contains("alice"),
        "diagnostic shape label should not expose raw author keys"
    );

    let remove_count = Arc::new(AtomicUsize::new(0));
    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();

    let batches = diagnostics.batches();
    assert_eq!(batches.len(), 2);
    let close = &batches[1];
    assert_eq!(
        close.reason.code,
        FeedSessionDiagnosticReasonCode::AcquisitionClose
    );
    assert_eq!(close.receipts.len(), 1);
    assert_eq!(
        close.receipts[0].event,
        FeedSessionDiagnosticEventKind::Close
    );
    assert_eq!(close.receipts[0].owner_counts.before, 1);
    assert_eq!(close.receipts[0].owner_counts.after, 0);
    assert!(
        close.receipts[0]
            .interest
            .as_ref()
            .and_then(|interest| interest.planner_interest_id_hint.as_ref())
            .is_some(),
        "close receipts retain the prior interest identity for relay correlation"
    );
}

#[test]
fn source_effect_diagnostics_are_marked_separately_from_initial_sync() {
    let (sender, rx) = command_receiver();
    let diagnostics = Arc::new(RecordingDiagnostics::default());
    let adapter = FeedSessionTrellisAdapter::new_with_diagnostics(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender,
        FeedSessionDiagnosticsHandle::new(diagnostics.clone()),
    )
    .unwrap();

    assert!(adapter.sync_source_effect_for_test(&extra(vec![interest("alice")]), "source-changed"));
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].2, "source-changed");

    let batches = diagnostics.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0].reason.code,
        FeedSessionDiagnosticReasonCode::SourceEffect
    );
}

#[test]
fn adapter_emits_precise_delta_only_for_trellis_resource_transitions() {
    let (sender, rx) = command_receiver();
    let adapter = FeedSessionTrellisAdapter::new(
        "app.feed.synthetic",
        FeedShape::Flat,
        vec![interest("fixed")],
        sender,
    )
    .unwrap();

    assert!(adapter.sync(&extra(Vec::new()), "source-changed"));
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(
        open_authors(&deltas[0].1),
        BTreeSet::from(["fixed".to_string()])
    );
    assert!(close_authors(&deltas[0].1).is_empty());

    assert!(adapter.sync(
        &extra(vec![interest("alice"), interest("bob")]),
        "source-changed"
    ));
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(
        open_authors(&deltas[0].1),
        BTreeSet::from(["alice".to_string(), "bob".to_string()])
    );
    assert!(close_authors(&deltas[0].1).is_empty());

    assert!(!adapter.sync(
        &extra(vec![interest("alice"), interest("bob")]),
        "source-changed"
    ));
    assert!(drain_deltas(&rx).is_empty());

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert!(open_authors(&deltas[0].1).is_empty());
    assert_eq!(
        close_authors(&deltas[0].1),
        BTreeSet::from(["bob".to_string()])
    );

    assert!(adapter.sync(&extra(Vec::new()), "source-changed"));
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert!(open_authors(&deltas[0].1).is_empty());
    assert_eq!(
        close_authors(&deltas[0].1),
        BTreeSet::from(["alice".to_string()])
    );
}

#[test]
fn close_scope_clears_once_and_late_source_effect_cannot_resurrect_demand() {
    let (sender, rx) = command_receiver();
    let adapter =
        FeedSessionTrellisAdapter::new("app.feed.synthetic", FeedShape::Flat, Vec::new(), sender)
            .unwrap();

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));
    assert_eq!(drain_deltas(&rx).len(), 1);

    let remove_count = Arc::new(AtomicUsize::new(0));
    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    let deltas = drain_deltas(&rx);
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    assert_eq!(deltas.len(), 1);
    assert_eq!(
        close_authors(&deltas[0].1),
        BTreeSet::from(["alice".to_string()])
    );
    assert_eq!(deltas[0].2, "feed-session-acquisition-close");

    assert!(!adapter.sync(&extra(vec![interest("bob")]), "source-changed"));
    assert!(drain_deltas(&rx).is_empty());

    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    assert!(drain_deltas(&rx).is_empty());
}

#[test]
fn adapter_output_lifecycle_frames_drive_rebaseline_and_clear() {
    let (sender, rx) = command_receiver();
    let adapter = FeedSessionTrellisAdapter::new(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender,
    )
    .unwrap();

    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![FeedSessionOutputFrameKind::Baseline]
    );
    assert!(!adapter.rebaseline_output_if_changed(false));
    assert_eq!(drain_mark_changed(&rx), 0);

    assert!(adapter.rebaseline_output_if_changed(true));
    assert_eq!(drain_mark_changed(&rx), 1);
    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![
            FeedSessionOutputFrameKind::Baseline,
            FeedSessionOutputFrameKind::Rebaseline,
        ]
    );

    let remove_count = Arc::new(AtomicUsize::new(0));
    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    let deltas = drain_deltas(&rx);
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    assert!(deltas.is_empty());
    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![
            FeedSessionOutputFrameKind::Baseline,
            FeedSessionOutputFrameKind::Rebaseline,
            FeedSessionOutputFrameKind::Clear,
        ]
    );

    assert!(!adapter.rebaseline_output_if_changed(true));
    assert_eq!(drain_mark_changed(&rx), 0);
}

#[test]
fn adapter_serializes_cross_thread_trellis_mutation() {
    let (sender, rx) = command_receiver();
    let adapter =
        FeedSessionTrellisAdapter::new("app.feed.synthetic", FeedShape::Flat, Vec::new(), sender)
            .unwrap();

    let callback_result = thread::spawn(move || {
        adapter.sync(
            &extra(vec![interest("wrong-thread")]),
            "wrong-thread-callback",
        );
    })
    .join();
    callback_result.expect("cross-thread Trellis mutation must serialize");

    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(
        open_authors(&deltas[0].1),
        BTreeSet::from(["wrong-thread".to_string()])
    );
}

#[test]
fn source_effect_callbacks_enqueue_actor_command_before_trellis_mutation() {
    let (sender, rx) = command_receiver();
    let adapter = FeedSessionTrellisAdapter::new(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender,
    )
    .unwrap();

    let callback_adapter = adapter.clone();
    let callback_result = thread::spawn(move || {
        callback_adapter.schedule_source_effect(
            extra(vec![interest("alice")]),
            "source-changed",
            true,
        );
    })
    .join();
    callback_result.expect("source-effect callbacks only enqueue actor commands");

    let mail = rx
        .try_recv()
        .expect("source-effect callback must enqueue a protocol command");
    let ActorMail::Command(ActorCommand::Protocol(_)) = mail else {
        panic!("unexpected actor mail");
    };
    assert!(rx.try_recv().is_err());
    assert_eq!(
        adapter.output_frame_kinds_for_test(),
        vec![FeedSessionOutputFrameKind::Baseline]
    );
}

fn remove_projection_action(count: Arc<AtomicUsize>) -> nmp_feed::TeardownAction {
    Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })
}
