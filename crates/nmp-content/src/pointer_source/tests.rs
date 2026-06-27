//! Unit tests for the pure pointer-source read model.

use nmp_core::substrate::KernelEvent;

use crate::embed_registry::EmbedTarget;

use super::{PointerSortMode, PointerSourceModel};

fn pointer(id: &str, author: &str, created_at: u64, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn target_event(id: &str, author: &str, kind: u32, created_at: u64, d: Option<&str>) -> KernelEvent {
    let mut tags = Vec::new();
    if let Some(d) = d {
        tags.push(vec!["d".to_string(), d.to_string()]);
    }
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn event_target(id: &str) -> EmbedTarget {
    EmbedTarget::Event(id.to_string())
}

#[test]
fn event_id_reference_materializes_one_target() {
    let mut model = PointerSourceModel::default();
    let changed = model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]]));
    assert!(changed, "first reference to a target widens demand");

    let demand: Vec<_> = model.target_demand().cloned().collect();
    assert_eq!(demand, vec![event_target("note1")]);
    assert_eq!(model.pointed_by(&event_target("note1")), vec!["p1".to_string()]);
}

#[test]
fn address_reference_materializes_address_target() {
    let mut model = PointerSourceModel::default();
    model.apply_pointer(&pointer(
        "p1",
        "alice",
        100,
        vec![vec!["a", "30023:bob:my-article"]],
    ));

    let demand: Vec<_> = model.target_demand().cloned().collect();
    assert_eq!(
        demand,
        vec![EmbedTarget::Address {
            kind: 30_023,
            pubkey: "bob".to_string(),
            identifier: "my-article".to_string(),
        }]
    );
}

#[test]
fn bare_and_malformed_references_fail_closed() {
    let mut model = PointerSourceModel::default();
    // Empty `e` value, non-addressable `a` coordinate, and a `p` tag: none are
    // hydratable target references, so demand stays empty (no wildcard query).
    let changed = model.apply_pointer(&pointer(
        "p1",
        "alice",
        100,
        vec![vec!["e", ""], vec!["a", "1:bob:x"], vec!["p", "carol"]],
    ));
    assert!(!changed);
    assert!(model.is_empty());
    assert_eq!(model.target_demand().len(), 0);
}

#[test]
fn redelivered_pointer_is_idempotent() {
    let mut model = PointerSourceModel::default();
    assert!(model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]])));
    assert!(
        !model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]])),
        "same pointer id must not re-widen demand"
    );
    assert_eq!(model.pointed_by(&event_target("note1")).len(), 1);
}

#[test]
fn cross_pointer_overlap_dedups_to_one_target() {
    let mut model = PointerSourceModel::default();
    assert!(model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]])));
    assert!(
        !model.apply_pointer(&pointer("p2", "bob", 101, vec![vec!["e", "note1"]])),
        "second pointer to an already-demanded target must not widen demand"
    );
    assert_eq!(model.target_demand().len(), 1);
    assert_eq!(model.pointed_by(&event_target("note1")).len(), 2);
}

#[test]
fn source_shrink_closes_unreferenced_target() {
    let mut model = PointerSourceModel::default();
    model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]]));
    model.apply_pointer(&pointer("p2", "bob", 101, vec![vec!["e", "note1"]]));

    // Dropping one of two pointers keeps the target demanded.
    assert!(!model.drop_pointer(&"p1".to_string()));
    assert_eq!(model.target_demand().len(), 1);

    // Dropping the last pointer withdraws the target entirely.
    assert!(model.drop_pointer(&"p2".to_string()));
    assert!(model.is_empty());
}

#[test]
fn target_hydration_only_accepts_demanded_targets() {
    let mut model = PointerSourceModel::default();
    model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]]));

    // An undemanded event is ignored.
    assert!(!model.apply_target(&target_event("other", "z", 1, 50, None)));
    assert!(model.items().is_empty());

    // The demanded target hydrates and projects.
    assert!(model.apply_target(&target_event("note1", "carol", 1, 90, None)));
    let items = model.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event.author, "carol");
    assert_eq!(items[0].pointer_count, 1);
}

#[test]
fn addressable_target_keeps_newest_version() {
    let mut model = PointerSourceModel::default();
    model.apply_pointer(&pointer(
        "p1",
        "alice",
        100,
        vec![vec!["a", "30023:bob:slug"]],
    ));

    assert!(model.apply_target(&target_event("v1", "bob", 30_023, 80, Some("slug"))));
    // Older version is rejected (newest-wins).
    assert!(!model.apply_target(&target_event("v0", "bob", 30_023, 70, Some("slug"))));
    // Newer version supersedes.
    assert!(model.apply_target(&target_event("v2", "bob", 30_023, 90, Some("slug"))));

    let items = model.items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event.id, "v2");
}

#[test]
fn sort_modes_order_projection() {
    let mut model = PointerSourceModel::default();
    // target A: 2 pointers from 2 authors, latest pointer at t=150, content t=10.
    model.apply_pointer(&pointer("pa1", "alice", 100, vec![vec!["e", "A"]]));
    model.apply_pointer(&pointer("pa2", "bob", 150, vec![vec!["e", "A"]]));
    // target B: 1 pointer, latest pointer at t=120, content t=90.
    model.apply_pointer(&pointer("pb1", "alice", 120, vec![vec!["e", "B"]]));
    model.apply_target(&target_event("A", "x", 1, 10, None));
    model.apply_target(&target_event("B", "y", 1, 90, None));

    let ids = |model: &PointerSourceModel| -> Vec<String> {
        model.items().into_iter().map(|item| item.event.id).collect()
    };

    model.set_sort(PointerSortMode::Time);
    assert_eq!(ids(&model), vec!["B", "A"], "newest content first");

    model.set_sort(PointerSortMode::TagTime);
    assert_eq!(ids(&model), vec!["A", "B"], "most-recently-pointed first");

    model.set_sort(PointerSortMode::Count);
    assert_eq!(ids(&model), vec!["A", "B"], "most-pointed first");

    model.set_sort(PointerSortMode::UniqueAuthor);
    assert_eq!(ids(&model), vec!["A", "B"], "most author-diverse first");
}

#[test]
fn set_sort_does_not_change_demand() {
    let mut model = PointerSourceModel::default();
    model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "note1"]]));
    model.apply_target(&target_event("note1", "carol", 1, 90, None));

    let demand_before: Vec<_> = model.target_demand().cloned().collect();
    assert!(model.set_sort(PointerSortMode::Count));
    assert!(!model.set_sort(PointerSortMode::Count), "no-op re-set");
    let demand_after: Vec<_> = model.target_demand().cloned().collect();

    assert_eq!(
        demand_before, demand_after,
        "sort changes must not touch the materialization demand (no reopen)"
    );
}

#[test]
fn unique_authors_distinct_from_count() {
    let mut model = PointerSourceModel::default();
    // 3 pointers, 2 distinct authors.
    model.apply_pointer(&pointer("p1", "alice", 100, vec![vec!["e", "A"]]));
    model.apply_pointer(&pointer("p2", "alice", 101, vec![vec!["e", "A"]]));
    model.apply_pointer(&pointer("p3", "bob", 102, vec![vec!["e", "A"]]));
    model.apply_target(&target_event("A", "x", 1, 10, None));

    let item = model.items().remove(0);
    assert_eq!(item.pointer_count, 3);
    assert_eq!(item.unique_authors, 2);
    assert_eq!(item.latest_pointer_at, 102);
}
