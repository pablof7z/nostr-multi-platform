use std::collections::{BTreeMap, BTreeSet};

use nmp_feed::{FeedShape, ProjectionKey};
use nmp_planner::{InterestLifecycle, InterestScope, InterestShape};
use trellis_core::{DependencyList, Graph, ResourceCommand, ResourceKey, ResourcePlan};

use super::trellis_resources::{
    FeedSessionResourceCommand, FeedSessionRouteProvenance, FeedSessionScopeKey,
    HostStatusIdentity, InterestDemand, InterestSetDemand, InterestSetReason, ProjectionAttachment,
    ReplayDemand,
};

fn author_shape(author: &str) -> InterestShape {
    InterestShape::timeline_for(
        BTreeSet::from([author.to_string()]),
        BTreeSet::from([1_u32, 6]),
    )
}

fn projection(value: &str) -> ProjectionKey {
    ProjectionKey::app_owned(value).expect("test projection key is app-owned")
}

fn active_follow(author: &str) -> InterestDemand {
    InterestDemand::tailing(
        &InterestScope::ActiveAccount,
        author_shape(author),
        FeedSessionRouteProvenance::ActiveFollowTimeline,
    )
}

fn trellis_key(key: &super::trellis_resources::FeedSessionResourceKey) -> ResourceKey {
    ResourceKey::new(key.as_str().to_string())
}

fn demand_map(
    values: &[InterestDemand],
) -> BTreeMap<super::trellis_resources::FeedSessionResourceKey, InterestDemand> {
    values
        .iter()
        .cloned()
        .map(|demand| (demand.resource_key(), demand))
        .collect()
}

#[test]
fn equivalent_interest_demand_shares_key_across_session_owners() {
    let first = active_follow("alice");
    let second = active_follow("alice");
    let owner_a = FeedSessionScopeKey::projection(&projection("app.timeline.a"));
    let owner_b = FeedSessionScopeKey::projection(&projection("app.timeline.b"));

    assert_eq!(first.resource_key(), second.resource_key());
    assert_ne!(owner_a, owner_b);
    assert!(first
        .resource_key()
        .as_str()
        .starts_with("nmp.feed-session.resource.v1:interest:"));
    assert!(owner_a
        .as_str()
        .starts_with("nmp.feed-session.scope.v1:projection="));
}

#[test]
fn semantically_distinct_interest_demand_gets_distinct_key() {
    let base = active_follow("alice");
    let different_author = active_follow("bob");
    let different_scope = InterestDemand::tailing(
        &InterestScope::Account("alice-pubkey".to_string()),
        author_shape("alice"),
        FeedSessionRouteProvenance::ActiveFollowTimeline,
    );
    let different_lifecycle = InterestDemand::new(
        &InterestScope::ActiveAccount,
        author_shape("alice"),
        InterestLifecycle::OneShot,
        FeedSessionRouteProvenance::ActiveFollowTimeline,
    );
    let different_provenance = InterestDemand::tailing(
        &InterestScope::ActiveAccount,
        author_shape("alice"),
        FeedSessionRouteProvenance::StaticFeedScope,
    );

    let keys = BTreeSet::from([
        base.resource_key().clone(),
        different_author.resource_key(),
        different_scope.resource_key(),
        different_lifecycle.resource_key(),
        different_provenance.resource_key(),
    ]);
    assert_eq!(keys.len(), 5);
}

#[test]
fn typed_payloads_keep_output_and_retry_policy_out_of_resource_identity() {
    let projection = projection("app.timeline");
    let owner = FeedSessionScopeKey::projection(&projection);
    let alice = active_follow("alice");
    let bob = active_follow("bob");
    let set = InterestSetDemand::new(
        owner.clone(),
        vec![bob.clone(), alice.clone()],
        InterestSetReason::SourceChanged,
    );
    let sorted_children: Vec<_> = set
        .children
        .iter()
        .map(InterestDemand::resource_key)
        .collect();
    let mut expected = sorted_children.clone();
    expected.sort();
    assert_eq!(sorted_children, expected);

    let attachment = ProjectionAttachment::new(projection.clone(), FeedShape::Flat);
    let replay = ReplayDemand::new(projection, vec![bob, alice.clone()]);
    assert_ne!(attachment.resource_key(), replay.resource_key());

    let status = HostStatusIdentity::new(alice.resource_key(), owner.clone(), 42);
    assert_eq!(
        status,
        HostStatusIdentity::new(alice.resource_key(), owner, 42),
        "status identity is resource key + scope + command revision only"
    );

    // `set` is the typed interest-set payload under test above; it no longer
    // wraps into a `FeedSessionResourceCommand` variant (`ReplaceInterestSet`
    // was deleted as dead staged code — #2631 closed, see #3116).
    let _ = set;
}

#[test]
fn trellis_shares_equivalent_feed_session_resources_until_last_owner_closes() {
    let demand = active_follow("alice");
    let key = trellis_key(&demand.resource_key());
    let mut graph = Graph::<FeedSessionResourceCommand>::new_with_command_type();

    let mut tx = graph.begin_transaction().unwrap();
    let scope_a = tx.create_scope("feed-session-a").unwrap();
    let scope_b = tx.create_scope("feed-session-b").unwrap();
    let input_a = tx
        .input::<BTreeMap<super::trellis_resources::FeedSessionResourceKey, InterestDemand>>(
            "session-a-demand",
        )
        .unwrap();
    let input_b = tx
        .input::<BTreeMap<super::trellis_resources::FeedSessionResourceKey, InterestDemand>>(
            "session-b-demand",
        )
        .unwrap();
    tx.set_input(input_a, BTreeMap::new()).unwrap();
    tx.set_input(input_b, BTreeMap::new()).unwrap();

    let demand_a = tx
        .map_collection(
            "session-a-resource-demand",
            DependencyList::new([input_a.id()]).unwrap(),
            move |ctx| Ok(ctx.input(input_a)?.clone()),
        )
        .unwrap();
    let demand_b = tx
        .map_collection(
            "session-b-resource-demand",
            DependencyList::new([input_b.id()]).unwrap(),
            move |ctx| Ok(ctx.input(input_b)?.clone()),
        )
        .unwrap();
    tx.map_resource_planner(demand_a, scope_a, move |ctx| {
        let mut plan = ResourcePlan::new();
        for added in &ctx.diff().added {
            let (key, demand) = &added.value;
            plan.open(
                trellis_key(key),
                ctx.scope(),
                FeedSessionResourceCommand::OpenInterest(demand.clone()),
            );
        }
        for removed in &ctx.diff().removed {
            let (key, _) = &removed.value;
            plan.close(trellis_key(key), ctx.scope());
        }
        Ok(plan)
    })
    .unwrap();
    tx.map_resource_planner(demand_b, scope_b, move |ctx| {
        let mut plan = ResourcePlan::new();
        for added in &ctx.diff().added {
            let (key, demand) = &added.value;
            plan.open(
                trellis_key(key),
                ctx.scope(),
                FeedSessionResourceCommand::OpenInterest(demand.clone()),
            );
        }
        for removed in &ctx.diff().removed {
            let (key, _) = &removed.value;
            plan.close(trellis_key(key), ctx.scope());
        }
        Ok(plan)
    })
    .unwrap();
    tx.commit().unwrap();
    drop(tx);

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(input_a, demand_map(&[demand.clone()]))
        .unwrap();
    let opened = tx.commit().unwrap();
    drop(tx);
    assert_eq!(
        opened.resource_plan.commands(),
        &[ResourceCommand::Open {
            key: key.clone(),
            scope: scope_a,
            command: FeedSessionResourceCommand::OpenInterest(demand.clone()),
        }]
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(input_b, demand_map(&[demand.clone()]))
        .unwrap();
    let shared = tx.commit().unwrap();
    drop(tx);
    assert!(shared.resource_plan.commands().is_empty());
    assert_eq!(
        graph.resource_owners(&key),
        Some(&BTreeSet::from([scope_a, scope_b]))
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(input_a, BTreeMap::new()).unwrap();
    let first_close = tx.commit().unwrap();
    drop(tx);
    assert!(first_close.resource_plan.commands().is_empty());
    assert_eq!(
        graph.resource_owners(&key),
        Some(&BTreeSet::from([scope_b]))
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(input_b, BTreeMap::new()).unwrap();
    let last_close = tx.commit().unwrap();
    drop(tx);
    assert_eq!(
        last_close.resource_plan.commands(),
        &[ResourceCommand::Close {
            key: key.clone(),
            scope: scope_b,
        }]
    );
    assert!(graph.resource_owners(&key).is_none());
}
