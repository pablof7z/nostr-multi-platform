use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};

use nmp_core::actor::{ActorCommand, ActorMail, InterestsCommand};
use nmp_core::subs::SubOwnerKey;
use nmp_core::{CommandSender, DependentInterestChild};
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

fn command_receiver() -> (CommandSender, mpsc::Receiver<ActorMail>) {
    let (tx, rx) = mpsc::channel();
    (CommandSender::new(tx), rx)
}

fn drain_replacements(
    rx: &mpsc::Receiver<ActorMail>,
) -> Vec<(SubOwnerKey, Vec<DependentInterestChild>, String)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|mail| match mail {
            ActorMail::Command(ActorCommand::Interests(
                InterestsCommand::ReplaceDependentInterestSet {
                    owner,
                    children,
                    reason,
                },
            )) => (owner, children, reason),
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

#[test]
fn adapter_emits_replacement_only_for_trellis_resource_transitions() {
    let (sender, rx) = command_receiver();
    let adapter =
        FeedSessionTrellisAdapter::new("app.feed.synthetic", vec![interest("fixed")], sender)
            .unwrap();

    assert!(adapter.sync(&extra(Vec::new()), "source-changed"));
    let replacements = drain_replacements(&rx);
    assert_eq!(replacements.len(), 1);
    assert_eq!(
        authors(&replacements[0].1),
        BTreeSet::from(["fixed".to_string()])
    );

    assert!(adapter.sync(
        &extra(vec![interest("alice"), interest("bob")]),
        "source-changed"
    ));
    let replacements = drain_replacements(&rx);
    assert_eq!(replacements.len(), 1);
    assert_eq!(
        authors(&replacements[0].1),
        BTreeSet::from(["alice".to_string(), "bob".to_string(), "fixed".to_string()])
    );

    assert!(!adapter.sync(
        &extra(vec![interest("alice"), interest("bob")]),
        "source-changed"
    ));
    assert!(drain_replacements(&rx).is_empty());

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));
    let replacements = drain_replacements(&rx);
    assert_eq!(replacements.len(), 1);
    assert_eq!(
        authors(&replacements[0].1),
        BTreeSet::from(["alice".to_string(), "fixed".to_string()])
    );

    assert!(adapter.sync(&extra(Vec::new()), "source-changed"));
    let replacements = drain_replacements(&rx);
    assert_eq!(replacements.len(), 1);
    assert_eq!(
        authors(&replacements[0].1),
        BTreeSet::from(["fixed".to_string()])
    );
}

#[test]
fn close_scope_clears_once_and_late_source_effect_cannot_resurrect_demand() {
    let (sender, rx) = command_receiver();
    let adapter = FeedSessionTrellisAdapter::new("app.feed.synthetic", Vec::new(), sender).unwrap();

    assert!(adapter.sync(&extra(vec![interest("alice")]), "source-changed"));
    assert_eq!(drain_replacements(&rx).len(), 1);

    (adapter.close_action())();
    let replacements = drain_replacements(&rx);
    assert_eq!(replacements.len(), 1);
    assert!(replacements[0].1.is_empty());
    assert_eq!(replacements[0].2, "feed-session-acquisition-close");

    assert!(!adapter.sync(&extra(vec![interest("bob")]), "source-changed"));
    assert!(drain_replacements(&rx).is_empty());
}
