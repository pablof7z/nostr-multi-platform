use std::collections::BTreeSet;

use trellis_core::{DependencyList, Graph, ResourceCommand, ResourceKey, ResourcePlan};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RelayDemand {
    OpenAuthorFeed { author: String },
}

fn authors(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn author_feed_key(author: &str) -> ResourceKey {
    ResourceKey::new(format!("author-feed:{author}"))
}

#[test]
fn trellis_source_shrink_withdraws_demand_and_empty_source_opens_none() {
    let mut graph = Graph::<RelayDemand>::new_with_command_type();
    let mut tx = graph.begin_transaction().unwrap();
    let scope = tx.create_scope("open feed view").unwrap();
    let source = tx
        .input::<BTreeSet<String>>("active source authors")
        .unwrap();
    tx.set_input(source, authors(&[])).unwrap();
    let demanded_authors = tx
        .set_collection(
            "author feed demand",
            DependencyList::new([source.id()]).unwrap(),
            move |ctx| Ok(ctx.input(source)?.clone()),
        )
        .unwrap();
    tx.set_resource_planner(demanded_authors, scope, move |ctx| {
        let mut plan = ResourcePlan::new();
        for added in &ctx.diff().added {
            plan.open(
                author_feed_key(&added.value),
                ctx.scope(),
                RelayDemand::OpenAuthorFeed {
                    author: added.value.clone(),
                },
            );
        }
        for removed in &ctx.diff().removed {
            plan.close(author_feed_key(&removed.value), ctx.scope());
        }
        Ok(plan)
    })
    .unwrap();
    let empty_source = tx.commit().unwrap();
    drop(tx);

    assert!(
        empty_source.resource_plan.commands().is_empty(),
        "an empty source must fail closed instead of opening wildcard demand"
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(source, authors(&["alice", "bob"])).unwrap();
    let populated_source = tx.commit().unwrap();
    drop(tx);

    assert_eq!(
        populated_source.resource_plan.commands(),
        &[
            ResourceCommand::Open {
                key: author_feed_key("alice"),
                scope,
                command: RelayDemand::OpenAuthorFeed {
                    author: "alice".to_owned(),
                },
            },
            ResourceCommand::Open {
                key: author_feed_key("bob"),
                scope,
                command: RelayDemand::OpenAuthorFeed {
                    author: "bob".to_owned(),
                },
            },
        ],
        "each source author opens exactly its corresponding relay demand"
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(source, authors(&["alice"])).unwrap();
    let shrunken_source = tx.commit().unwrap();
    drop(tx);

    assert_eq!(
        shrunken_source.resource_plan.commands(),
        &[ResourceCommand::Close {
            key: author_feed_key("bob"),
            scope,
        }],
        "shrinking the source withdraws demand for removed authors"
    );

    let mut tx = graph.begin_transaction().unwrap();
    tx.set_input(source, authors(&[])).unwrap();
    let cleared_source = tx.commit().unwrap();
    drop(tx);

    assert_eq!(
        cleared_source.resource_plan.commands(),
        &[ResourceCommand::Close {
            key: author_feed_key("alice"),
            scope,
        }],
        "clearing the source only closes remaining demand and opens nothing"
    );
}
