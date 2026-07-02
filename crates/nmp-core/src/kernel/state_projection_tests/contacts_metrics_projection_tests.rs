//! kind:3 contacts stay out of the `metrics` projection — no second
//! follow-count source of truth.

use super::projection_fixtures_support::{snapshot, ACCOUNT, FOLLOW_A, FOLLOW_B};
use crate::kernel::{Kernel, NostrEvent};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Active kind:3 ingest does not create a second follow-count source in metrics.
#[test]
fn contact_list_does_not_create_snapshot_metrics_source_of_truth() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    // Cold snapshot: no kind:3 → zero followed authors projected.
    let before = snapshot(&mut kernel);
    assert_eq!(
        before["metrics"]["contacts_authors"].as_u64(),
        Some(0),
        "before any kind:3 the projected contacts_authors count must be zero",
    );

    let event = NostrEvent {
        id: "0000000000000000000000000000000000000000000000000000000000000030".to_string(),
        pubkey: ACCOUNT.to_string(),
        created_at: 1_700_000_000,
        kind: 3,
        tags: vec![
            vec!["p".to_string(), FOLLOW_A.to_string()],
            vec!["p".to_string(), FOLLOW_B.to_string()],
        ],
        content: String::new(),
        sig: String::new(),
    };
    kernel.inject_contacts(event);

    let after = snapshot(&mut kernel);
    assert_eq!(
        after["metrics"]["contacts_authors"].as_u64(),
        Some(0),
        "metrics.contacts_authors is retired; follows derive from stored kind:3 at the read seam",
    );
    // Core contact ingest no longer owns feed author expansion; reduced feed
    // sources compile that dynamic author set above the generic interest seam.
    assert_eq!(
        after["metrics"]["timeline_authors"].as_u64(),
        Some(0),
        "active-account kind:3 must not mutate metrics.timeline_authors directly",
    );
}
