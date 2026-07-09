//! #3116 equivalence: full-recompute oracle + converged claims parity.
//!
//! Split out of `feed_author_refs_tests.rs` (which owns the pre-migration
//! behavioral spec) so the file-size hard cap wasn't tripped by adding this
//! test; both files share `feed_author_refs_support.rs`'s fixtures.
//!
//! Two feed consumers sharing an author must reconcile independently — the
//! composite `(consumer_id, author_key)` key the migration onto
//! `KeyedReconciler` relies on (#3116 module docs): an author stays claimed
//! until BOTH consumers drop it, exactly matching the pre-migration
//! per-consumer hand-rolled diff. Every step also asserts Trellis's own
//! `FullRecomputeCheck` oracle (`full_recompute_matches`) — the leak-audit
//! guarantee #3115/#3116 wires into every migrated reconciler.

use std::collections::BTreeSet;

use super::super::*;
use super::feed_author_consumer_id;
use super::feed_author_refs_support::{
    hex64, kernel_with_slot, register_swappable_provider, set_authors,
};

#[test]
fn full_recompute_oracle_and_converged_claims_across_two_overlapping_consumers() {
    let (mut kernel, slot) = kernel_with_slot();
    let feed_one = "test.feed.one";
    let feed_two = "test.feed.two";
    let authors_one = register_swappable_provider(&slot, feed_one);
    let authors_two = register_swappable_provider(&slot, feed_two);
    let consumer_one = feed_author_consumer_id(feed_one);
    let consumer_two = feed_author_consumer_id(feed_two);

    let a = hex64("a1111");
    let b = hex64("b2222");
    let c = hex64("c3333");

    // Step 0: consumer_one={a,b}, consumer_two={b,c} — `b` is shared.
    set_authors(&authors_one, &[a.clone(), b.clone()]);
    set_authors(&authors_two, &[b.clone(), c.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert_eq!(
        kernel.profile_claims.get(&a).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_one.clone()])
    );
    assert_eq!(
        kernel.profile_claims.get(&b).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_one.clone(), consumer_two.clone()]),
        "b is claimed by BOTH consumers (shared demand)"
    );
    assert_eq!(
        kernel.profile_claims.get(&c).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_two.clone()])
    );

    // Step 1: consumer_one drops `a` (now {b}); consumer_two unchanged. `b`
    // must survive — consumer_two still holds it.
    set_authors(&authors_one, &[b.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert!(
        !kernel.profile_claims.contains_key(&a),
        "a's only owner dropped it"
    );
    assert_eq!(
        kernel.profile_claims.get(&b).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_one.clone(), consumer_two.clone()]),
        "b survives while consumer_two still holds it"
    );

    // Step 2: consumer_one drops everything (now {}); consumer_two unchanged
    // {b,c}. `b` must STILL survive — consumer_two is its last owner now.
    set_authors(&authors_one, &[]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert_eq!(
        kernel.profile_claims.get(&b).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_two.clone()]),
        "b narrows to consumer_two's sole ownership, not torn down"
    );
    assert!(
        kernel
            .auto_profile_refs_by_consumer
            .get(&consumer_one)
            .is_none(),
        "consumer_one's tracking entry is gone once its set is empty"
    );

    // Step 3: consumer_two drops `b` too (now {c}) — `b`'s LAST owner
    // releases, so the slot tears down.
    set_authors(&authors_two, &[c.clone()]);
    kernel.reconcile_feed_author_refs();
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert!(
        !kernel.profile_claims.contains_key(&b),
        "b's last owner released it"
    );
    assert_eq!(
        kernel.profile_claims.get(&c).cloned().unwrap_or_default(),
        BTreeSet::from([consumer_two.clone()])
    );

    // Step 4: reconcile the SAME desired set again — a genuine no-op must not
    // touch anything (idempotent, no refcount inflation).
    let claims_before = kernel.profile_claims.clone();
    kernel.reconcile_feed_author_refs();
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert_eq!(
        kernel.profile_claims, claims_before,
        "no-op reconcile changes nothing"
    );

    // Closing consumer_two releases its last ref (`c`) — everything drains.
    kernel.release_all_feed_author_refs(&consumer_two);
    assert!(kernel.feed_author_reconciler.full_recompute_matches());
    assert!(
        kernel.profile_claims.is_empty(),
        "release-all drains the last consumer's claims"
    );
}
