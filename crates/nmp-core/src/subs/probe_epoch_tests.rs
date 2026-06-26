//! B3 (Workstream B acquisition-one-door) — indexer-lane outage epoch tests.
//!
//! Unit coverage for [`SubscriptionLifecycle::note_indexer_lane_recovered`] and
//! [`SubscriptionLifecycle::probe_epoch`]: the lane-level edge gate that re-arms
//! the implicit kind:10002 discovery probe set (`probed_mailboxes`) ONLY on a
//! genuine full-indexer-lane outage recovery, never on routine per-socket churn.
//! Split out of `lifecycle_tests.rs` (file-size hard ceiling) — `use super::*;`
//! resolves to the `subs` module exactly as the sibling test modules do.

use super::*;

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// The cold-start first connect (`indexer_lane_down` starts `false`) is an
/// `up → up` no-op: the epoch stays 0 and the probed set is untouched. The
/// cold-start probe is the normal first recompile's job, NOT this edge.
#[test]
fn first_indexer_connect_does_not_bump_epoch() {
    let mut l = SubscriptionLifecycle::new();
    l.probed_mailboxes.insert(pubkey("a"));
    assert_eq!(l.probe_epoch(), 0, "epoch starts at 0");

    let re_armed = l.note_indexer_lane_recovered(true);

    assert!(!re_armed, "the first connect must not re-arm");
    assert_eq!(
        l.probe_epoch(),
        0,
        "the first connect must not bump the epoch"
    );
    assert_eq!(
        l.probed_mailboxes().len(),
        1,
        "the first connect must NOT clear the probed set (no churn at startup)",
    );
    assert!(
        l.inbox.is_empty(),
        "the first connect must enqueue no recompile trigger",
    );
}

/// A genuine full-lane outage recovery (`down → up`) bumps the epoch ONCE,
/// re-arms the probed set, and enqueues a single `IndexerSetChanged` carrying
/// the new epoch as its generation.
#[test]
fn full_lane_outage_recovery_bumps_epoch_and_rearms() {
    let mut l = SubscriptionLifecycle::new();
    l.probed_mailboxes.insert(pubkey("a"));
    l.probed_mailboxes.insert(pubkey("b"));

    // Lane is up at first (cold-start connect: no-op).
    assert!(!l.note_indexer_lane_recovered(true));
    // Every indexer drops → lane down (records the outage edge, no re-arm).
    let down = l.note_indexer_lane_recovered(false);
    assert!(!down, "going down must not re-arm");
    assert_eq!(l.probe_epoch(), 0, "going down must not bump the epoch");
    assert_eq!(
        l.probed_mailboxes().len(),
        2,
        "going down must not clear the probed set (nothing to re-probe while down)",
    );

    // Drain the inbox so we can assert the recovery enqueues exactly one trigger.
    let _ = l.inbox.drain_coalesced();

    // An indexer comes back → genuine recovery: bump + re-arm + one trigger.
    let recovered = l.note_indexer_lane_recovered(true);
    assert!(recovered, "the down → up recovery must re-arm");
    assert_eq!(l.probe_epoch(), 1, "the recovery must bump the epoch to 1");
    assert!(
        l.probed_mailboxes().is_empty(),
        "the recovery must clear the probed set so still-uncached authors re-probe",
    );
    let drained = l.inbox.drain_coalesced();
    assert!(
        drained.iter().any(|t| matches!(
            t,
            CompileTrigger::IndexerSetChanged { generation } if *generation == 1
        )),
        "the recovery must enqueue IndexerSetChanged carrying the new epoch; got {drained:?}",
    );
}

/// A reconnect while ≥1 indexer stayed connected (`up → up`) is a no-op — the
/// regression the per-socket gate caused (a single flapping indexer re-blasting
/// the whole probe batch while siblings were live). The lane never went down,
/// so the epoch is stable and the probed set survives.
#[test]
fn sibling_still_live_reconnect_does_not_rearm() {
    let mut l = SubscriptionLifecycle::new();
    l.probed_mailboxes.insert(pubkey("a"));

    // Cold-start connect (no-op), then a stream of `up` observations: a sibling
    // indexer flapped (its own connect fired `note_..(true)`) but at least one
    // indexer was connected the whole time, so the lane never went down.
    assert!(!l.note_indexer_lane_recovered(true));
    for _ in 0..5 {
        let re_armed = l.note_indexer_lane_recovered(true);
        assert!(!re_armed, "an up → up observation must never re-arm");
    }

    assert_eq!(
        l.probe_epoch(),
        0,
        "no full outage occurred → epoch unchanged"
    );
    assert_eq!(
        l.probed_mailboxes().len(),
        1,
        "the probed set must survive sibling-still-live reconnect churn",
    );
    assert!(
        l.inbox.is_empty(),
        "sibling-still-live reconnects must enqueue no recompile trigger",
    );
}

/// Repeated full outages each advance the epoch by exactly one — the epoch is a
/// monotonic "indexer outages survived" counter.
#[test]
fn repeated_outages_advance_epoch_monotonically() {
    let mut l = SubscriptionLifecycle::new();
    assert!(!l.note_indexer_lane_recovered(true)); // cold start
    for expected in 1..=3u64 {
        assert!(!l.note_indexer_lane_recovered(false), "down edge");
        assert!(l.note_indexer_lane_recovered(true), "recovery edge");
        assert_eq!(
            l.probe_epoch(),
            expected,
            "each outage→recovery cycle must bump the epoch by one",
        );
    }
}
