use super::*;
use crate::{
    interest::InterestShape,
    plan::{canonical_filter_hash, RelayPlan, SubShape},
};

fn plan_with_sources(relays: &[(&str, &[&str], &[RoutingSource])]) -> CompiledPlan {
    let mut per_relay = BTreeMap::new();
    for (relay, authors, sources) in relays {
        let mut shape = InterestShape::default();
        for author in *authors {
            shape.authors.insert((*author).to_string());
        }
        let canonical_filter_hash = canonical_filter_hash(&shape);
        let sub_shape = SubShape {
            shape,
            originating_interests: vec![],
            canonical_filter_hash,
        };
        per_relay.insert(
            (*relay).to_string(),
            RelayPlan {
                relay_url: (*relay).to_string(),
                role_tags: sources.iter().cloned().collect(),
                sub_shapes: vec![sub_shape],
            },
        );
    }
    CompiledPlan {
        plan_id: "hint-selection".to_string(),
        per_relay,
        unroutable_authors: BTreeSet::new(),
    }
}

#[test]
fn hint_relay_survives_even_when_nip65_budget_already_covers_author() {
    let mut plan = plan_with_sources(&[
        ("wss://atlas.example", &["gigi"], &[RoutingSource::Nip65]),
        ("wss://eden.example", &["gigi"], &[RoutingSource::Nip65]),
        ("wss://hint.example", &["gigi"], &[RoutingSource::Hint]),
    ]);

    apply_selection(&mut plan, 30, 2);

    assert!(
        plan.per_relay.contains_key("wss://hint.example"),
        "hint relay must survive selection; got {:?}",
        plan.per_relay.keys().collect::<Vec<_>>()
    );
}

#[test]
fn provenance_and_user_hint_relays_survive_selection() {
    let mut plan = plan_with_sources(&[
        ("wss://atlas.example", &["gigi"], &[RoutingSource::Nip65]),
        ("wss://eden.example", &["gigi"], &[RoutingSource::Nip65]),
        (
            "wss://seen.example",
            &["gigi"],
            &[RoutingSource::Provenance],
        ),
        (
            "wss://user-hint.example",
            &["gigi"],
            &[RoutingSource::UserConfigured(UserConfiguredCategory::Debug)],
        ),
    ]);

    apply_selection(&mut plan, 30, 2);

    assert!(plan.per_relay.contains_key("wss://seen.example"));
    assert!(plan.per_relay.contains_key("wss://user-hint.example"));
}
