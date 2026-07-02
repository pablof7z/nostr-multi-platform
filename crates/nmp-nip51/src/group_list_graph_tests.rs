use std::collections::BTreeSet;

use super::*;

const ALICE: &str = "alice";
const BOB: &str = "bob";

fn groups(values: &[(&str, &str)]) -> BTreeSet<SimpleGroupRef> {
    values
        .iter()
        .map(|(local_id, relay)| SimpleGroupRef::new(*local_id, *relay))
        .collect()
}

fn visible_effect(effects: Vec<SimpleGroupListGraphEffect>) -> Option<BTreeSet<SimpleGroupRef>> {
    match effects.as_slice() {
        [] => None,
        [SimpleGroupListGraphEffect::PerspectiveChanged { groups }] => Some(groups.clone()),
        _ => panic!("simple-group graph emits at most one perspective effect"),
    }
}

fn upsert_oracle(
    store: &mut SimpleGroupListGraphStore,
    owner: &str,
    groups: BTreeSet<SimpleGroupRef>,
    created_at: u64,
) {
    if store.owner_pubkey.as_deref() != Some(owner) {
        store.owner_pubkey = Some(owner.to_string());
        store.groups.clear();
        store.created_at = 0;
    }
    if created_at < store.created_at {
        return;
    }
    store.groups = groups;
    store.created_at = created_at;
}

fn assert_matches_full_recompute(
    graph: &SimpleGroupListGraph,
    active: Option<&str>,
    store: &SimpleGroupListGraphStore,
) {
    assert_eq!(
        graph.current_visible_groups(),
        visible_groups(active, store)
    );
}

#[test]
fn trellis_simple_group_graph_matches_full_recompute_across_source_prefixes() {
    let mut active = Some(ALICE.to_string());
    let mut graph = SimpleGroupListGraph::new(active.clone());
    let mut store = SimpleGroupListGraphStore::default();
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    let room_a = groups(&[("room-a", "wss://relay-a")]);
    upsert_oracle(&mut store, ALICE, room_a.clone(), 10);
    let expected = visible_groups(active.as_deref(), &store);
    let effect = visible_effect(graph.upsert_list(ALICE.to_string(), room_a.clone(), 10));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, room_a.clone(), 20);
    let effect = visible_effect(graph.upsert_list(ALICE.to_string(), room_a, 20));
    assert_eq!(
        effect, None,
        "newer same-group events advance source truth without visible churn"
    );
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    let room_b = groups(&[("room-b", "wss://relay-b")]);
    upsert_oracle(&mut store, ALICE, room_b.clone(), 15);
    let effect = visible_effect(graph.upsert_list(ALICE.to_string(), room_b.clone(), 15));
    assert_eq!(effect, None, "stale replaceable echoes are no-ops");
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    upsert_oracle(&mut store, ALICE, room_b.clone(), 30);
    let expected = visible_groups(active.as_deref(), &store);
    let effect = visible_effect(graph.upsert_list(ALICE.to_string(), room_b, 30));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    let room_c = groups(&[("room-c", "wss://relay-c")]);
    upsert_oracle(&mut store, BOB, room_c.clone(), 40);
    let expected = visible_groups(active.as_deref(), &store);
    let effect = visible_effect(graph.upsert_list(BOB.to_string(), room_c, 40));
    assert_eq!(effect, Some(expected), "owner switch hides stale groups");
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    active = Some(BOB.to_string());
    let expected = visible_groups(active.as_deref(), &store);
    let effect = visible_effect(graph.apply_active_source(active.clone()));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);

    active = None;
    let expected = visible_groups(active.as_deref(), &store);
    let effect = visible_effect(graph.apply_active_source(active.clone()));
    assert_eq!(effect, Some(expected));
    assert_matches_full_recompute(&graph, active.as_deref(), &store);
}
