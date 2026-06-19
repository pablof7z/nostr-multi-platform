//! ADR-0058 §6 step-4 — Protected-cursor retention-claim publish wiring.
//!
//! The kernel is the single writer of the store's log-retention claim set:
//! after every register / advance / unregister it rebuilds the claims from its
//! registry and calls `EventStore::replace_log_retention_claims`. These tests
//! observe that wiring END-TO-END through the store's append-time log trim:
//!
//!   - register Protected → the published claim pins the log floor so an append
//!     that would normally advance the floor leaves it where the cursor sits;
//!   - unregister → the claim is cleared, so the next append trims normally
//!     (the pinned tail becomes a `PullGap`);
//!   - advance → the claim's `after_seq` moves forward, releasing the rows the
//!     cursor has consumed so normal trimming resumes.
//!
//! The store is driven to the `DEFAULT_LOG_MAX_ENTRIES` boundary with cheap
//! kind:1 inserts so the normal floor is live without any white-box poking.

use std::num::NonZeroUsize;

use super::pull::{PullLimits, PullScope};
use super::pull_cursor::{PullCursorId, PullCursorMode};
use super::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::ingest_log::DEFAULT_LOG_MAX_ENTRIES;
use crate::store::{RawEvent, ScanLogResult, VerifiedEvent};

const RELAY: &str = "wss://t/";

fn new_kernel() -> Kernel {
    Kernel::new(DEFAULT_VISIBLE_LIMIT)
}

fn limits() -> PullLimits {
    PullLimits {
        max_entries: NonZeroUsize::new(64).unwrap(),
        max_scan_entries: NonZeroUsize::new(256).unwrap(),
    }
}

/// A unique, structurally-valid kind:1 event for ingest counter `n`.
fn raw(n: u64) -> RawEvent {
    RawEvent {
        id: format!("{n:064x}"),
        pubkey: "aa".repeat(32),
        created_at: n + 1,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "cc".repeat(64),
    }
}

/// Insert `count` fresh events, advancing the shared id counter.
fn insert_n(k: &Kernel, counter: &mut u64, count: u64) {
    let handle = k.event_store_handle();
    for _ in 0..count {
        handle
            .insert(
                VerifiedEvent::from_raw_unchecked(raw(*counter)),
                &RELAY.to_string(),
                0,
            )
            .unwrap();
        *counter += 1;
    }
}

fn floor_is_zero_seq1_available(k: &Kernel) -> bool {
    matches!(
        k.event_store_handle().scan_log_since_seq(0, 4).unwrap(),
        ScanLogResult::Page(_)
    )
}

fn scan_zero_is_gap(k: &Kernel) -> bool {
    matches!(
        k.event_store_handle().scan_log_since_seq(0, 4).unwrap(),
        ScanLogResult::Gap(_)
    )
}

/// Register a Protected cursor pinned at `after_seq` with an effectively
/// unbounded lag, then assert an append leaves the floor pinned; unregister and
/// assert the next append trims normally.
#[test]
fn register_protected_pins_floor_and_unregister_releases() {
    let mut k = new_kernel();
    let mut ctr = 0u64;

    // Fill the log exactly to the bound: floor stays 0, latest == MAX.
    insert_n(&k, &mut ctr, DEFAULT_LOG_MAX_ENTRIES);
    assert_eq!(
        k.event_store_handle().latest_ingest_seq().unwrap(),
        DEFAULT_LOG_MAX_ENTRIES
    );
    assert!(
        floor_is_zero_seq1_available(&k),
        "floor 0 before any extra append"
    );

    // Register a Protected cursor at after_seq=0 → publishes a claim pinning
    // the floor to 0. The next append would normally advance the floor to 1.
    k.register_pull_cursor(
        PullCursorId(1),
        "mirror".to_string(),
        PullScope::GlobalLog,
        PullCursorMode::Protected {
            max_lag_entries: u64::MAX,
        },
        0,
        limits(),
    );
    insert_n(&k, &mut ctr, 1); // latest = MAX + 1
    assert!(
        floor_is_zero_seq1_available(&k),
        "Protected claim must pin the floor so scan from 0 stays a Page"
    );
    assert_eq!(
        k.event_store_handle().oldest_available_seq().unwrap(),
        Some(1),
        "the pinned tail row (seq 1) must still be available"
    );

    // Unregister → claim cleared. The next append trims to the normal floor.
    k.unregister_pull_cursor(PullCursorId(1));
    insert_n(&k, &mut ctr, 1); // latest = MAX + 2 → normal floor advances past seq 0
    assert!(
        scan_zero_is_gap(&k),
        "after unregister the claim is gone → normal trim → scan from 0 is a Gap"
    );
}

/// Advancing a Protected cursor moves its claim's `after_seq` forward, releasing
/// the consumed tail so the floor can advance normally.
#[test]
fn advance_protected_moves_pin_forward() {
    let mut k = new_kernel();
    let mut ctr = 0u64;

    insert_n(&k, &mut ctr, DEFAULT_LOG_MAX_ENTRIES); // latest == MAX, floor 0

    k.register_pull_cursor(
        PullCursorId(7),
        "mirror".to_string(),
        PullScope::GlobalLog,
        PullCursorMode::Protected {
            max_lag_entries: u64::MAX,
        },
        0,
        limits(),
    );
    insert_n(&k, &mut ctr, 1); // latest = MAX + 1, pinned at 0
    assert!(floor_is_zero_seq1_available(&k), "pinned at after_seq=0");

    // Consumer drains to the head: advance the claim's after_seq forward.
    let head = k.event_store_handle().latest_ingest_seq().unwrap();
    k.advance_pull_cursor(PullCursorId(7), head);
    insert_n(&k, &mut ctr, 1); // latest = MAX + 2
    assert!(
        scan_zero_is_gap(&k),
        "advancing the cursor released the consumed tail → normal trim → Gap from 0"
    );
}
