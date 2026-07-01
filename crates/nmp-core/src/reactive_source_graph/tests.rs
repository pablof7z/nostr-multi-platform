use std::collections::BTreeSet;

use super::{GraphError, ReactiveSourceGraph, SourceInputUpdate, SourceNodeId};

#[derive(Clone, Debug, Eq, PartialEq)]
enum TestEffect {
    ReplaceAuthors(Vec<String>),
    Sum(i32),
    Reset,
}

fn id(value: &str) -> SourceNodeId {
    SourceNodeId::from(value)
}

fn authors(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn active_account_chain_fails_closed_and_suppresses_no_ops() {
    let active = id("active-account");
    let follows = id("active-follows");
    let effect = id("acquisition-effect");
    let mut graph = ReactiveSourceGraph::<TestEffect>::new();

    graph
        .add_input(active.clone(), Option::<String>::None)
        .unwrap();
    graph
        .add_derived::<BTreeSet<String>, _>(follows.clone(), [active.clone()], {
            let active = active.clone();
            move |read| match read.get::<Option<String>>(&active) {
                Some(Some(pubkey)) => authors(&[pubkey.as_str()]),
                _ => BTreeSet::new(),
            }
        })
        .unwrap();
    graph
        .add_effect(effect.clone(), [follows.clone()], {
            let follows = follows.clone();
            move |read| {
                let authors = read
                    .get::<BTreeSet<String>>(&follows)
                    .expect("follows exists")
                    .iter()
                    .cloned()
                    .collect();
                Some(TestEffect::ReplaceAuthors(authors))
            }
        })
        .unwrap();

    let turn = graph
        .set_input(active.clone(), Some("alice".to_string()))
        .unwrap();
    assert_eq!(
        turn.effects(),
        &[TestEffect::ReplaceAuthors(vec!["alice".to_string()])]
    );
    assert_eq!(
        graph.get::<BTreeSet<String>>(&follows),
        Some(&authors(&["alice"]))
    );

    let turn = graph
        .set_input(active.clone(), Some("alice".to_string()))
        .unwrap();
    assert!(turn.is_empty(), "same value must not emit false wakes");

    let turn = graph.set_input(active, Option::<String>::None).unwrap();
    assert_eq!(turn.effects(), &[TestEffect::ReplaceAuthors(Vec::new())]);
    assert_eq!(
        graph.get::<BTreeSet<String>>(&follows),
        Some(&BTreeSet::new())
    );
    assert_eq!(graph.revision(&effect).expect("effect rev").get(), 2);
}

#[test]
fn batched_inputs_coalesce_downstream_effects_once() {
    let a = id("a");
    let b = id("b");
    let sum = id("sum");
    let effect = id("sum-effect");
    let mut graph = ReactiveSourceGraph::<TestEffect>::new();

    graph.add_input(a.clone(), 0_i32).unwrap();
    graph.add_input(b.clone(), 0_i32).unwrap();
    graph
        .add_derived::<i32, _>(sum.clone(), [a.clone(), b.clone()], {
            let a = a.clone();
            let b = b.clone();
            move |read| read.get::<i32>(&a).unwrap() + read.get::<i32>(&b).unwrap()
        })
        .unwrap();
    graph
        .add_effect(effect, [sum.clone()], {
            let sum = sum.clone();
            move |read| Some(TestEffect::Sum(*read.get::<i32>(&sum).unwrap()))
        })
        .unwrap();

    let turn = graph
        .apply_inputs([
            SourceInputUpdate::new(a, 1_i32),
            SourceInputUpdate::new(b, 2_i32),
        ])
        .unwrap();

    assert_eq!(graph.get::<i32>(&sum), Some(&3));
    assert_eq!(turn.effects(), &[TestEffect::Sum(3)]);
}

#[test]
fn unchanged_derived_value_blocks_downstream_effect() {
    let raw = id("raw");
    let parity = id("parity");
    let effect = id("parity-effect");
    let mut graph = ReactiveSourceGraph::<TestEffect>::new();

    graph.add_input(raw.clone(), 1_i32).unwrap();
    graph
        .add_derived::<bool, _>(parity.clone(), [raw.clone()], {
            let raw = raw.clone();
            move |read| read.get::<i32>(&raw).unwrap() % 2 != 0
        })
        .unwrap();
    graph
        .add_effect(effect, [parity.clone()], |_| Some(TestEffect::Reset))
        .unwrap();

    let turn = graph.set_input(raw.clone(), 3_i32).unwrap();
    assert_eq!(graph.get::<bool>(&parity), Some(&true));
    assert!(turn.effects().is_empty());
    assert_eq!(turn.changed_nodes().len(), 1);

    let turn = graph.set_input(raw, 4_i32).unwrap();
    assert_eq!(graph.get::<bool>(&parity), Some(&false));
    assert_eq!(turn.effects(), &[TestEffect::Reset]);
}

#[test]
fn registration_and_input_type_errors_are_explicit() {
    let input = id("input");
    let missing = id("missing");
    let mut graph = ReactiveSourceGraph::<TestEffect>::new();

    graph.add_input(input.clone(), 1_i32).unwrap();
    assert!(matches!(
        graph.add_input(input.clone(), 2_i32),
        Err(GraphError::DuplicateNode(id)) if id == input
    ));
    assert!(matches!(
        graph.add_derived::<i32, _>(id("bad-derived"), [missing.clone()], |_| 0),
        Err(GraphError::MissingDependency { dependency, .. }) if dependency == missing
    ));
    assert!(matches!(
        graph.set_input(input.clone(), true),
        Err(GraphError::ValueTypeMismatch { node, .. }) if node == input
    ));
}
