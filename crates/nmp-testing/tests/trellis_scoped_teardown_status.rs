use std::collections::BTreeSet;

use trellis_core::{
    DependencyList, Graph, HostResourceOutcome, HostResourceStatus, ResourceCommand, ResourceKey,
    ResourcePlan, Revision,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RelayDemand {
    OpenSubscription { filter_json: String },
}

fn resource_key(relay_url: &str, filter_json: &str) -> ResourceKey {
    ResourceKey::new(format!("relay-subscription:{relay_url}:{filter_json}"))
}

fn demand(relay_url: &str, filter_json: &str) -> BTreeSet<(String, String)> {
    BTreeSet::from([(relay_url.to_owned(), filter_json.to_owned())])
}

#[test]
fn scoped_teardown_then_stale_host_open_does_not_resurrect_demand() {
    let relay_url = "wss://relay.example";
    let filter_json = r#"{"authors":["alice"],"kinds":[1]}"#;
    let key = resource_key(relay_url, filter_json);

    let mut graph = Graph::<RelayDemand>::new_with_command_type();
    let mut tx = graph.begin_transaction().unwrap();
    let scope = tx.create_scope("nmp.timeline.read-session").unwrap();
    let requested = tx
        .input::<BTreeSet<(String, String)>>("rust-owned-demand-source")
        .unwrap();
    let host_statuses = tx
        .input::<Vec<HostResourceStatus>>("host-resource-status-feedback")
        .unwrap();
    tx.set_input(requested, demand(relay_url, filter_json))
        .unwrap();
    tx.set_input(host_statuses, Vec::new()).unwrap();
    tx.attach_node_to_scope(requested, scope).unwrap();
    // Host feedback is external process state; it may arrive after scoped demand is reclaimed.

    let demand_collection = tx
        .set_collection(
            "scoped-relay-demand",
            DependencyList::new([requested.id()]).unwrap(),
            move |ctx| Ok(ctx.input(requested)?.clone()),
        )
        .unwrap();
    tx.set_resource_planner(demand_collection, scope, move |ctx| {
        let mut plan = ResourcePlan::new();
        for added in &ctx.diff().added {
            let (relay_url, filter_json) = &added.value;
            plan.open(
                resource_key(relay_url, filter_json),
                ctx.scope(),
                RelayDemand::OpenSubscription {
                    filter_json: filter_json.clone(),
                },
            );
        }
        for removed in &ctx.diff().removed {
            let (relay_url, filter_json) = &removed.value;
            plan.close(resource_key(relay_url, filter_json), ctx.scope());
        }
        Ok(plan)
    })
    .unwrap();

    let opened = tx.commit().unwrap();
    drop(tx);

    assert_eq!(
        opened.resource_plan.commands(),
        &[ResourceCommand::Open {
            key: key.clone(),
            scope,
            command: RelayDemand::OpenSubscription {
                filter_json: filter_json.to_owned(),
            },
        }]
    );
    assert_eq!(graph.resource_owners(&key), Some(&BTreeSet::from([scope])));

    let mut tx = graph.begin_transaction().unwrap();
    tx.close_scope(scope).unwrap();
    let closed = tx.commit().unwrap();
    drop(tx);

    assert_eq!(
        closed.resource_plan.commands(),
        &[ResourceCommand::Close {
            key: key.clone(),
            scope,
        }]
    );
    assert!(graph.resource_owners(&key).is_none());

    let stale_open = HostResourceStatus::new(
        key.clone(),
        scope,
        opened.revision,
        Revision::new(1),
        HostResourceOutcome::Open,
    );
    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(host_statuses, vec![stale_open.clone()])
        .unwrap();
    let stale = tx.commit().unwrap();
    drop(tx);

    assert!(
        stale.resource_plan.commands().is_empty(),
        "host status is feedback only; it must not reopen a closed scope"
    );
    assert!(
        graph.resource_owners(&key).is_none(),
        "stale host Open cannot resurrect closed Trellis demand"
    );
    assert_eq!(
        graph.input_value(host_statuses).unwrap(),
        Some(&vec![stale_open]),
        "the stale status remains recorded as canonical input"
    );
}
