use super::*;
use crate::{
    interest::InterestShape,
    plan::{canonical_filter_hash, RelayPlan, SubShape},
};

/// Helper that builds a per-relay sub-shape tagged with the supplied routing
/// source set. Mirrors `tests::plan_with` but lets the test pin the lane each
/// relay landed on, since app-relay protection is keyed on `role_tags`.
fn plan_with_sources(relays: &[(&str, &[&str], &[RoutingSource])]) -> CompiledPlan {
    let mut per_relay = BTreeMap::new();
    for (relay, authors, sources) in relays {
        let mut shape = InterestShape::default();
        for a in *authors {
            shape.authors.insert((*a).to_string());
        }
        let hash = canonical_filter_hash(&shape);
        let sub = SubShape {
            shape,
            originating_interests: vec![],
            canonical_filter_hash: hash,
        };
        let mut role_tags = BTreeSet::new();
        for src in *sources {
            role_tags.insert(src.clone());
        }
        per_relay.insert(
            (*relay).to_string(),
            RelayPlan {
                relay_url: (*relay).to_string(),
                role_tags,
                sub_shapes: vec![sub],
            },
        );
    }
    CompiledPlan {
        plan_id: "test".to_string(),
        per_relay,
        unroutable_authors: BTreeSet::new(),
    }
}

#[test]
fn app_relay_survives_selection_even_when_outbox_already_covers_author() {
    // Gallery-TUI smoke repro: 1 author, outbox=[atlas, eden], app_relays=[primal].
    // Under DEFAULT_SELECT_MAX_PER_USER=2 the greedy pass picks atlas+eden
    // and drops primal unless the AppRelay lane bypasses selection.
    let nip65 = [RoutingSource::Nip65];
    let app = [RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay,
    )];
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["gigi"], &nip65),
        ("wss://relay.primal.net", &["gigi"], &app),
    ]);
    apply_selection(&mut plan, 30, 2);

    assert!(
        plan.per_relay.contains_key("wss://relay.primal.net"),
        "primal (AppRelay-tagged) must survive selection; got relays: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
    let primal = &plan.per_relay["wss://relay.primal.net"];
    assert_eq!(primal.sub_shapes.len(), 1);
    assert!(
        primal.sub_shapes[0].shape.authors.contains("gigi"),
        "app relay must carry the author on its sub-shape; got {:?}",
        primal.sub_shapes[0].shape.authors,
    );
}

#[test]
fn app_relay_survives_even_under_tight_max_connections_budget() {
    let nip65 = [RoutingSource::Nip65];
    let app = [RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay,
    )];
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["gigi"], &nip65),
        ("wss://relay.primal.net", &["gigi"], &app),
    ]);
    apply_selection(&mut plan, 1, 2);

    assert!(
        plan.per_relay.contains_key("wss://relay.primal.net"),
        "primal must survive even when max_connections=1; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
}

#[test]
fn dual_lane_app_relay_plus_nip65_survives() {
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &[RoutingSource::Nip65]),
        ("wss://eden.nostr.land", &["gigi"], &[RoutingSource::Nip65]),
        (
            "wss://relay.primal.net",
            &["gigi"],
            &[
                RoutingSource::Nip65,
                RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay),
            ],
        ),
    ]);
    apply_selection(&mut plan, 30, 2);

    assert!(
        plan.per_relay.contains_key("wss://relay.primal.net"),
        "dual-lane primal must survive; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
}

#[test]
fn non_app_relay_lanes_are_still_subject_to_coverage_pruning() {
    let nip65 = [RoutingSource::Nip65];
    let indexer = [RoutingSource::UserConfigured(
        UserConfiguredCategory::Indexer,
    )];
    let mut plan = plan_with_sources(&[
        ("wss://atlas.nostr.land", &["gigi"], &nip65),
        ("wss://eden.nostr.land", &["gigi"], &nip65),
        ("wss://relay.primal.net", &["gigi"], &indexer),
    ]);
    apply_selection(&mut plan, 30, 2);

    assert!(
        !plan.per_relay.contains_key("wss://relay.primal.net"),
        "Indexer-tagged relay must remain subject to coverage pruning; got: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );
}
