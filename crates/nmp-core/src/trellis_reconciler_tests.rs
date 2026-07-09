//! Boundary proofs for [`super::KeyedReconciler`]: opening N members emits N
//! ordered `Open`s, reconcile touches only the delta (an untouched member is
//! never closed+reopened), a payload change on a live key emits `Replace`
//! (never `Close`+`Open`), close drains every live member in LIFO order, and
//! Trellis's own `FullRecomputeCheck` oracle agrees with incremental state
//! at every step.

use std::collections::BTreeMap;

use trellis_core::{ResourceCommand, ResourceKey};

use super::KeyedReconciler;

fn reconciler() -> KeyedReconciler<String, u32> {
    KeyedReconciler::new("test-scope", |key: &String| ResourceKey::new(key.clone()))
        .expect("fresh reconciler")
}

fn desired(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), *value))
        .collect()
}

fn opened_keys(commands: &[ResourceCommand<u32>]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            ResourceCommand::Open { key, .. } => Some(key.as_str().to_string()),
            _ => None,
        })
        .collect()
}

fn closed_keys(commands: &[ResourceCommand<u32>]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            ResourceCommand::Close { key, .. } => Some(key.as_str().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn first_reconcile_opens_every_desired_member() {
    let reconciler = reconciler();
    let commands = reconciler.reconcile(desired(&[("a", 1), ("b", 2)]));
    assert_eq!(opened_keys(&commands), vec!["a", "b"]);
    assert!(reconciler.full_recompute_matches());
}

#[test]
fn reconcile_adds_a_new_member_without_touching_the_existing_one() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1), ("b", 2)]));

    let commands = reconciler.reconcile(desired(&[("a", 1), ("b", 2), ("c", 3)]));
    assert_eq!(opened_keys(&commands), vec!["c"]);
    assert!(
        closed_keys(&commands).is_empty(),
        "adding a member must not close any existing member's interest"
    );
    assert!(reconciler.full_recompute_matches());
}

#[test]
fn reconcile_closes_a_member_no_longer_desired() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1), ("b", 2)]));

    let commands = reconciler.reconcile(desired(&[("b", 2)]));
    assert_eq!(closed_keys(&commands), vec!["a"]);
    assert!(
        opened_keys(&commands).is_empty(),
        "no re-open of the still-desired member"
    );
    assert!(reconciler.full_recompute_matches());
}

#[test]
fn reconcile_replaces_a_live_member_whose_payload_changed() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1)]));

    let commands = reconciler.reconcile(desired(&[("a", 2)]));
    assert_eq!(commands.len(), 1, "a payload change is one Replace: {commands:?}");
    match &commands[0] {
        ResourceCommand::Replace { key, command, .. } => {
            assert_eq!(key.as_str(), "a");
            assert_eq!(*command, 2);
        }
        other => panic!("expected Replace, got {other:?}"),
    }
    assert!(reconciler.full_recompute_matches());
}

#[test]
fn reconcile_leaves_an_unchanged_member_untouched() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1)]));

    let commands = reconciler.reconcile(desired(&[("a", 1)]));
    assert!(
        commands.is_empty(),
        "an unchanged key/payload pair must not emit any command: {commands:?}"
    );
    assert!(reconciler.full_recompute_matches());
}

#[test]
fn close_drains_every_member_in_reverse_acquisition_order() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1)]));
    let _ = reconciler.reconcile(desired(&[("a", 1), ("b", 2)]));
    let _ = reconciler.reconcile(desired(&[("a", 1), ("b", 2), ("c", 3)]));

    let commands = reconciler.close();
    assert_eq!(
        closed_keys(&commands),
        vec!["c", "b", "a"],
        "LIFO: the most-recently-acquired member closes first (trellis-core's scope-close ordering guarantee)"
    );
}

#[test]
fn close_is_idempotent() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1)]));
    let first = reconciler.close();
    assert_eq!(closed_keys(&first), vec!["a"]);

    let second = reconciler.close();
    assert!(second.is_empty(), "a second close is a no-op, never a panic");
}

#[test]
fn reconcile_after_close_is_a_no_op() {
    let reconciler = reconciler();
    let _ = reconciler.reconcile(desired(&[("a", 1)]));
    let _ = reconciler.close();

    let commands = reconciler.reconcile(desired(&[("z", 9)]));
    assert!(
        commands.is_empty(),
        "a closed reconciler ignores further reconcile calls"
    );
}
