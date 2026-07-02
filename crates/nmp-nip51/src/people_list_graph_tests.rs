use std::collections::{BTreeMap, BTreeSet};

use super::*;

const ALICE: &str = "alice";
const BOB: &str = "bob";
const CAROL: &str = "carol";

fn members(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn visible_effect(
    effects: Vec<PeopleListGraphEffect>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    match effects.as_slice() {
        [] => None,
        [PeopleListGraphEffect::PerspectiveChanged { lists }] => Some(lists.clone()),
        _ => panic!("people-list graph emits at most one perspective effect"),
    }
}

fn upsert_oracle(
    store: &mut PeopleListGraphStore,
    owner: &str,
    list_id: &str,
    members: BTreeSet<String>,
    created_at: u64,
) {
    if store.owner_pubkey.as_deref() != Some(owner) {
        store.owner_pubkey = Some(owner.to_string());
        store.lists.clear();
    }
    if store
        .lists
        .get(list_id)
        .is_some_and(|existing| created_at < existing.created_at)
    {
        return;
    }
    store.lists.insert(
        list_id.to_string(),
        PeopleListGraphEntry {
            members,
            created_at,
        },
    );
}

fn assert_matches_full_recompute(
    graph: &PeopleListGraph,
    active: Option<&str>,
    store: &PeopleListGraphStore,
) {
    assert_eq!(graph.current_visible_lists(), visible_lists(active, store));
}

#[test]
fn trellis_people_graph_matches_full_recompute_across_source_prefixes() {
    let mut active = Some(ALICE.to_string());
    let mut graph = PeopleListGraph::new(active.clone());
    let mut store = PeopleListGraphStore::default();
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, "team", members(&[BOB]), 10);
    let expected = visible_lists(active.as_deref(), &store);
    let effect = visible_effect(graph.upsert_list(
        ALICE.to_string(),
        "team".to_string(),
        members(&[BOB]),
        10,
    ));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, "team", members(&[BOB]), 20);
    let effect = visible_effect(graph.upsert_list(
        ALICE.to_string(),
        "team".to_string(),
        members(&[BOB]),
        20,
    ));
    assert_eq!(
        effect, None,
        "newer same-member events advance source truth without visible churn"
    );
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, "team", members(&[CAROL]), 15);
    let effect = visible_effect(graph.upsert_list(
        ALICE.to_string(),
        "team".to_string(),
        members(&[CAROL]),
        15,
    ));
    assert_eq!(effect, None, "stale replaceable echoes are no-ops");
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, "team", members(&[CAROL]), 30);
    let expected = visible_lists(active.as_deref(), &store);
    let effect = visible_effect(graph.upsert_list(
        ALICE.to_string(),
        "team".to_string(),
        members(&[CAROL]),
        30,
    ));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, BOB, "team", members(&[BOB]), 40);
    let expected = visible_lists(active.as_deref(), &store);
    let effect =
        visible_effect(graph.upsert_list(BOB.to_string(), "team".to_string(), members(&[BOB]), 40));
    assert_eq!(effect, Some(expected), "owner switch hides stale lists");
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    active = Some(BOB.to_string());
    let expected = visible_lists(active.as_deref(), &store);
    let effect = visible_effect(graph.apply_active_source(active.clone()));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    active = None;
    let expected = visible_lists(active.as_deref(), &store);
    let effect = visible_effect(graph.apply_active_source(active.clone()));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);
}
