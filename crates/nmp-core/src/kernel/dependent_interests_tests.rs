use super::{
    DependentInterestChild, DependentInterestDelta, DependentInterestDeltaCommand, Kernel,
};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::subs::{SubKey, SubOwnerKey, SubScope};
use std::collections::BTreeSet;

fn hex(seed: u8) -> String {
    format!("{seed:064x}")
}

fn child_with_key(key: SubKey, id: u64, authors: &[String]) -> DependentInterestChild {
    DependentInterestChild {
        key,
        scope: SubScope::Global,
        interest: LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().cloned().collect(),
                kinds: BTreeSet::from([1]),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        },
    }
}

fn child(name: &'static str, id: u64, authors: &[String]) -> DependentInterestChild {
    child_with_key(SubKey::new(name), id, authors)
}

fn pending_triggers(kernel: &Kernel) -> usize {
    kernel.lifecycle.pending_trigger_count()
}

#[test]
fn typed_tailing_child_matches_open_interest_key_for_active_and_global() {
    let shape = InterestShape::timeline_for(BTreeSet::from([hex(42)]), BTreeSet::from([1, 6]));
    for (scope, raw_scope) in [
        (InterestScope::ActiveAccount, 0_u32),
        (InterestScope::Global, 1_u32),
    ] {
        let child = DependentInterestChild::tailing(shape.clone(), scope.clone());
        let filter_json = crate::subs::wire::filter_json_for(&shape);
        let (identity, interest) = crate::subs::interest_builder::build_interest_pair(
            &filter_json,
            "consumer",
            raw_scope,
            None,
            false,
            crate::planner::InterestLifecycle::Tailing,
        )
        .expect("canonical filter parses");

        assert_eq!(child.key, identity.key);
        assert_eq!(child.scope, identity.scope);
        assert_eq!(child.interest.id, interest.id);
        assert_eq!(child.interest.id, InterestId(child.key.0));
        assert_eq!(child.interest.scope, interest.scope);
        assert_eq!(child.interest.shape, interest.shape);
        assert_eq!(child.interest.lifecycle, InterestLifecycle::Tailing);
    }
}

#[test]
fn replace_add_shrink_replace_and_empty_fail_closed() {
    let mut kernel = Kernel::testing_new(80);
    let owner = SubOwnerKey::new("source-owner");
    let author_a = hex(1);
    let author_b = hex(2);
    let author_c = hex(3);
    let child_a = child("child-a", 1, std::slice::from_ref(&author_a));
    let child_b = child("child-b", 2, std::slice::from_ref(&author_b));

    let before = pending_triggers(&kernel);
    let outcome = kernel.replace_dependent_interest_set(
        owner,
        vec![child_a.clone(), child_b.clone()],
        "test-dependent-add",
    );
    assert_eq!(outcome.registered_children, 2);
    assert_eq!(outcome.withdrawn_children, 0);
    assert_eq!(outcome.changed_registrations, 2);
    assert_eq!(kernel.lifecycle.registry().len(), 2);
    assert_eq!(pending_triggers(&kernel) - before, 1);

    let child_a_reduced = child("child-a", 3, std::slice::from_ref(&author_c));
    let before = pending_triggers(&kernel);
    let outcome = kernel.replace_dependent_interest_set(
        owner,
        vec![child_a_reduced.clone()],
        "test-dependent-shrink",
    );
    assert_eq!(outcome.registered_children, 1);
    assert_eq!(outcome.withdrawn_children, 1);
    assert_eq!(outcome.closed_slots, 1);
    assert_eq!(outcome.changed_registrations, 1);
    assert_eq!(kernel.lifecycle.registry().len(), 1);
    let active = kernel.lifecycle.registry().iter_active();
    assert_eq!(active[0].shape.authors, BTreeSet::from([author_c]));
    assert_eq!(pending_triggers(&kernel) - before, 1);

    let before = pending_triggers(&kernel);
    let outcome = kernel.replace_dependent_interest_set(owner, Vec::new(), "test-dependent-empty");
    assert_eq!(outcome.registered_children, 0);
    assert_eq!(outcome.withdrawn_children, 1);
    assert_eq!(outcome.closed_slots, 1);
    assert_eq!(outcome.changed_registrations, 0);
    assert!(kernel.lifecycle.registry().is_empty());
    assert!(!kernel.dependent_interest_sets.contains_key(&owner));
    assert_eq!(pending_triggers(&kernel) - before, 1);
}

#[test]
fn shared_child_dedups_until_last_source_owner_closes() {
    let mut kernel = Kernel::testing_new(80);
    let owner_a = SubOwnerKey::new("source-a");
    let owner_b = SubOwnerKey::new("source-b");
    let shared_key = SubKey::new("shared-child");
    let shared = child_with_key(shared_key, 10, &[hex(10)]);

    let before = pending_triggers(&kernel);
    let first = kernel.replace_dependent_interest_set(
        owner_a,
        vec![shared.clone()],
        "test-dependent-first-owner",
    );
    assert_eq!(first.changed_registrations, 1);
    assert_eq!(pending_triggers(&kernel) - before, 1);

    let before = pending_triggers(&kernel);
    let second =
        kernel.replace_dependent_interest_set(owner_b, vec![shared], "test-dependent-second-owner");
    assert_eq!(second.changed_registrations, 0);
    assert_eq!(kernel.lifecycle.registry().len(), 1);
    assert_eq!(
        kernel
            .lifecycle
            .registry()
            .owner_count(&SubScope::Global, &shared_key),
        2
    );
    assert_eq!(pending_triggers(&kernel) - before, 0);

    let before = pending_triggers(&kernel);
    let first_close =
        kernel.replace_dependent_interest_set(owner_a, Vec::new(), "test-dependent-first-close");
    assert_eq!(first_close.withdrawn_children, 1);
    assert_eq!(first_close.closed_slots, 0);
    assert_eq!(kernel.lifecycle.registry().len(), 1);
    assert_eq!(
        kernel
            .lifecycle
            .registry()
            .owner_count(&SubScope::Global, &shared_key),
        1
    );
    assert_eq!(pending_triggers(&kernel) - before, 0);

    let before = pending_triggers(&kernel);
    let last_close =
        kernel.replace_dependent_interest_set(owner_b, Vec::new(), "test-dependent-last-close");
    assert_eq!(last_close.withdrawn_children, 1);
    assert_eq!(last_close.closed_slots, 1);
    assert!(kernel.lifecycle.registry().is_empty());
    assert_eq!(pending_triggers(&kernel) - before, 1);
}

#[test]
fn delta_opens_replaces_and_closes_without_full_owner_replacement() {
    let mut kernel = Kernel::testing_new(80);
    let owner = SubOwnerKey::new("source-owner");
    let other_owner = SubOwnerKey::new("other-source");
    let shared_key = SubKey::new("shared-child");
    let author_a = hex(1);
    let author_b = hex(2);
    let child_a = child_with_key(shared_key, 1, std::slice::from_ref(&author_a));
    let child_b = child_with_key(shared_key, 2, std::slice::from_ref(&author_b));
    let unrelated = child("unrelated", 3, &[hex(3)]);

    kernel.replace_dependent_interest_set(
        other_owner,
        vec![unrelated.clone()],
        "test-unrelated-owner",
    );
    let before = pending_triggers(&kernel);
    let opened = kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![DependentInterestDeltaCommand::Open(child_a.clone())],
        },
        "test-delta-open",
    );
    assert_eq!(opened.registered_children, 1);
    assert_eq!(opened.changed_registrations, 1);
    assert_eq!(kernel.lifecycle.registry().len(), 2);
    assert_eq!(pending_triggers(&kernel) - before, 1);

    let before = pending_triggers(&kernel);
    let replaced = kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![DependentInterestDeltaCommand::Replace(child_b.clone())],
        },
        "test-delta-replace",
    );
    assert_eq!(replaced.registered_children, 1);
    assert_eq!(replaced.changed_registrations, 1);
    let active = kernel.lifecycle.registry().iter_active();
    assert!(
        active
            .iter()
            .any(|interest| interest.shape.authors == BTreeSet::from([author_b.clone()]))
    );
    assert_eq!(pending_triggers(&kernel) - before, 1);

    let before = pending_triggers(&kernel);
    let closed = kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![DependentInterestDeltaCommand::Close(child_b)],
        },
        "test-delta-close",
    );
    assert_eq!(closed.withdrawn_children, 1);
    assert_eq!(closed.closed_slots, 1);
    assert_eq!(kernel.lifecycle.registry().len(), 1);
    assert_eq!(
        kernel.lifecycle.registry().owner_count(
            &unrelated.scope,
            &unrelated.key,
        ),
        1
    );
    assert_eq!(pending_triggers(&kernel) - before, 1);
}

#[test]
fn delta_with_open_before_close_withdraws_before_upserting() {
    let mut kernel = Kernel::testing_new(80);
    let owner = SubOwnerKey::new("source-owner");
    let shared_key = SubKey::new("follow-feed");
    let author_a = hex(1);
    let author_b = hex(2);
    let child_a = child_with_key(shared_key, 1, std::slice::from_ref(&author_a));
    let child_b = child_with_key(shared_key, 2, std::slice::from_ref(&author_b));

    kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![DependentInterestDeltaCommand::Open(child_a.clone())],
        },
        "test-delta-open",
    );

    let outcome = kernel.apply_dependent_interest_delta(
        owner,
        DependentInterestDelta {
            commands: vec![
                DependentInterestDeltaCommand::Open(child_b.clone()),
                DependentInterestDeltaCommand::Close(child_a),
            ],
        },
        "test-delta-replace-order",
    );

    assert_eq!(outcome.withdrawn_children, 1);
    assert_eq!(outcome.closed_slots, 1);
    assert_eq!(outcome.registered_children, 1);
    let active = kernel.lifecycle.registry().iter_active();
    assert!(
        active
            .iter()
            .all(|interest| !interest.shape.authors.contains(&author_a)),
        "closed author must not survive an open-before-close delta"
    );
    assert!(
        active
            .iter()
            .any(|interest| interest.shape.authors == BTreeSet::from([author_b.clone()]))
    );
}
