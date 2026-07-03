//! Kernel-side tests for the `OpenInterest` / `CloseInterest` dispatch
//! arms: newly-installed interest enqueues exactly one recompile trigger,
//! dedups do not re-enqueue, and a final close enqueues a teardown trigger.
//!
//! The six registry/builder tests (parse → shape, dedup, last-close, etc.)
//! live in their canonical home: `crates/nmp-core/src/subs/interest_builder.rs`.
//! The copies that previously lived here have been deleted (B5 hygiene).

use super::build_open_interest;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn open_interest_sub_installs_and_enqueues_trigger() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let before = kernel.lifecycle_mut().pending_trigger_count();

    let (identity, interest) = build_open_interest(
        r#"{"kinds":[1,6],"authors":["aa"]}"#,
        "author-aa",
        0,
        None,
        false,
        crate::planner::InterestLifecycle::Tailing,
    )
    .unwrap();
    let newly_installed = kernel.open_interest_sub(identity, interest);

    assert!(newly_installed, "first open installs the slot");
    assert_eq!(
        kernel.lifecycle_mut().pending_trigger_count(),
        before + 1,
        "a newly-installed interest enqueues exactly one recompile trigger"
    );
}

#[test]
fn open_interest_sub_dedup_does_not_re_enqueue() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;

    let (id1, int1) = build_open_interest(filter, "consumer-1", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    assert!(kernel.open_interest_sub(id1, int1));
    let after_first = kernel.lifecycle_mut().pending_trigger_count();

    // Second owner on the SAME (scope,key) slot: attaches but does NOT
    // re-install, so no second trigger (idempotent — would otherwise churn
    // the compiler on every re-mount).
    let (id2, int2) = build_open_interest(filter, "consumer-2", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    assert!(
        !kernel.open_interest_sub(id2, int2),
        "second owner attaches"
    );
    assert_eq!(
        kernel.lifecycle_mut().pending_trigger_count(),
        after_first,
        "attaching a second owner must not re-enqueue a trigger"
    );
}

#[test]
fn close_interest_sub_enqueues_trigger_only_on_last_owner() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let filter = r#"{"kinds":[1,6],"authors":["aa"]}"#;

    let (id1, int1) = build_open_interest(filter, "consumer-1", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    let (id2, int2) = build_open_interest(filter, "consumer-2", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    kernel.open_interest_sub(id1, int1);
    kernel.open_interest_sub(id2, int2);
    let after_opens = kernel.lifecycle_mut().pending_trigger_count();

    // First close: slot survives (consumer-2 still attached) → no trigger.
    let (close1, _) = build_open_interest(filter, "consumer-1", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    assert!(!kernel.close_interest_sub(&close1), "slot survives");
    assert_eq!(
        kernel.lifecycle_mut().pending_trigger_count(),
        after_opens,
        "a non-final close does not enqueue a trigger"
    );

    // Last close: slot dropped → exactly one trigger.
    let (close2, _) = build_open_interest(filter, "consumer-2", 0, None, false, crate::planner::InterestLifecycle::Tailing).unwrap();
    assert!(kernel.close_interest_sub(&close2), "last close drops slot");
    assert_eq!(
        kernel.lifecycle_mut().pending_trigger_count(),
        after_opens + 1,
        "the final close enqueues exactly one recompile trigger"
    );
}
