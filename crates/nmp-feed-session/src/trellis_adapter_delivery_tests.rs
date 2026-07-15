use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

use nmp_core::actor::{ActorCommand, ActorMail, InterestsCommand, LifecycleCommand};
use nmp_core::{
    CommandSendStatus, CommandSender, DependentInterestChild, DependentInterestDeltaCommand,
};
use nmp_feed::FeedShape;
use nmp_planner::InterestShape;

use crate::source::{AcquisitionInterest, ExtraAcquisition};
use crate::trellis_adapter::FeedSessionTrellisAdapter;
use crate::trellis_resources::FeedSessionRouteProvenance;

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

fn bounded_command_receiver(capacity: usize) -> (CommandSender, mpsc::Receiver<ActorMail>) {
    CommandSender::bounded_channel_with_capacity(capacity)
}

fn fill_command_queue(sender: &CommandSender) {
    assert_eq!(
        sender
            .send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
            .expect("bounded inbox should still be connected"),
        CommandSendStatus::Enqueued
    );
}

fn drain_filler(rx: &mpsc::Receiver<ActorMail>) {
    match rx
        .try_recv()
        .expect("filler command should still be queued")
    {
        ActorMail::Command(ActorCommand::Lifecycle(LifecycleCommand::Shutdown)) => {}
        other => panic!("expected filler shutdown command, got {other:?}"),
    }
}

fn drain_deltas(rx: &mpsc::Receiver<ActorMail>) -> Vec<Vec<DependentInterestDeltaCommand>> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|mail| match mail {
            ActorMail::Command(ActorCommand::Interests(
                InterestsCommand::ApplyDependentInterestDelta { delta, .. },
            )) => delta.commands,
            _ => panic!("unexpected actor mail"),
        })
        .collect()
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

fn remove_projection_action(count: Arc<AtomicUsize>) -> nmp_feed::TeardownAction {
    Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })
}

#[test]
fn bounded_inbox_dropped_open_delta_retries_on_unchanged_desired_state() {
    let (sender, rx) = bounded_command_receiver(1);
    fill_command_queue(&sender);
    let adapter = FeedSessionTrellisAdapter::new(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender.clone(),
    )
    .unwrap();

    assert!(
        !adapter.sync(&extra(vec![interest("alice")]), "source-changed"),
        "a delta dropped by the bounded actor inbox is not delivered yet"
    );
    assert_eq!(sender.command_drops(), 1);
    drain_filler(&rx);
    assert!(drain_deltas(&rx).is_empty());

    assert!(
        adapter.sync(&extra(vec![interest("alice")]), "source-changed"),
        "unchanged desired state must retry the pending delta"
    );
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(open_authors(&deltas[0]), BTreeSet::from(["alice".into()]));
}

#[test]
fn bounded_inbox_dropped_close_delta_keeps_output_removal_retryable() {
    let (sender, rx) = bounded_command_receiver(1);
    let adapter = FeedSessionTrellisAdapter::new(
        "app.feed.synthetic",
        FeedShape::Flat,
        Vec::new(),
        sender.clone(),
    )
    .unwrap();

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));
    assert_eq!(drain_deltas(&rx).len(), 1);

    fill_command_queue(&sender);
    let remove_count = Arc::new(AtomicUsize::new(0));
    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    assert_eq!(sender.command_drops(), 1);
    assert_eq!(
        remove_count.load(Ordering::SeqCst),
        0,
        "output removal must wait while dependent-interest teardown is pending"
    );
    drain_filler(&rx);
    assert!(drain_deltas(&rx).is_empty());

    (adapter.close_action(remove_projection_action(Arc::clone(&remove_count))))();
    assert_eq!(remove_count.load(Ordering::SeqCst), 1);
    let deltas = drain_deltas(&rx);
    assert_eq!(deltas.len(), 1);
    assert_eq!(close_authors(&deltas[0]), BTreeSet::from(["alice".into()]));
}
