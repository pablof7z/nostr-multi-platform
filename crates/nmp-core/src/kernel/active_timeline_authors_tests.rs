//! Tests for the public `Kernel::active_timeline_authors` accessor (V-59
//! rung 1, #1).

use super::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::collections::BTreeSet;

const HOST_DECLARED_FOLLOW_FEED_KIND: u32 = 4242;

fn hex_pk(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

fn activate_follow_feed(kernel: &mut Kernel) {
    kernel.follow_feed_kinds = BTreeSet::from([HOST_DECLARED_FOLLOW_FEED_KIND]);
}

/// An empty timeline-author set yields an empty `Vec`.
#[test]
fn active_timeline_authors_is_empty_by_default() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        kernel.active_timeline_authors().is_empty(),
        "a fresh kernel has no timeline authors"
    );
}

/// The accessor mirrors the projection that `sync_follow_feed_interests`
/// rebuilds: the active account's follow set ∪ the active account itself,
/// returned as a sorted `Vec` of raw hex pubkeys.
#[test]
fn active_timeline_authors_reflects_synced_follow_set() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let me = hex_pk("a");
    let alice = hex_pk("b");
    let bob = hex_pk("c");
    kernel.active_account = Some(me.clone());
    activate_follow_feed(&mut kernel);

    kernel.sync_follow_feed_interests(&[bob.clone(), alice.clone()]);

    let authors = kernel.active_timeline_authors();
    // BTreeSet backing → ascending sort. me=a…, alice=b…, bob=c… so the
    // sorted order is [me, alice, bob].
    assert_eq!(
        authors,
        vec![me.clone(), alice.clone(), bob.clone()],
        "accessor must return the follow set ∪ the active account, sorted, raw"
    );
}

/// The accessor returns an owned clone — the result is independent of the
/// kernel's backing store (mutating the kernel afterwards does not retro-
/// actively change a previously returned `Vec`).
#[test]
fn active_timeline_authors_returns_owned_snapshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let me = hex_pk("a");
    kernel.active_account = Some(me.clone());
    activate_follow_feed(&mut kernel);
    kernel.sync_follow_feed_interests(&[hex_pk("b")]);

    let snapshot = kernel.active_timeline_authors();
    assert_eq!(snapshot.len(), 2);

    // Re-sync with a smaller set; the previously captured Vec is unaffected.
    kernel.sync_follow_feed_interests(&[]);
    assert_eq!(
        snapshot.len(),
        2,
        "a previously returned Vec is an owned snapshot, not a live view"
    );
    assert_eq!(
        kernel.active_timeline_authors(),
        vec![me],
        "after re-sync the accessor reflects only the active account"
    );
}
