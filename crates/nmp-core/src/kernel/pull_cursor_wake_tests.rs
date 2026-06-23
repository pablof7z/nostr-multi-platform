//! Tests for the ADR-0058 §10 step-3a pull-cursor wake + registry path.
//!
//! Covers register/advance/unregister wake arming, the level-triggered re-arm
//! in `drain_pull_wakes`, coalescing, the `MAX_PULL_CURSORS` cap (loud no-op),
//! and the D8 guarantee that wakes appear ONLY via `StoreWakeups` arms (no
//! timer / sleep / poll spontaneously produces one).

use std::num::NonZeroUsize;

use super::pull::{PullLimits, PullScope};
use super::pull_cursor::{
    PullConsumerId, PullCursorHandle, PullCursorId, PullCursorMode, PullCursorSpec,
    MAX_PULL_CURSORS,
};
use super::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex64(byte: u8) -> String {
    format!("{:02x}", byte).repeat(32)
}

fn raw(id_byte: u8, ts: u64) -> RawEvent {
    RawEvent {
        id: hex64(id_byte),
        pubkey: hex64(0xAA),
        created_at: ts,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "cc".repeat(64),
    }
}

fn new_kernel() -> Kernel {
    Kernel::new(DEFAULT_VISIBLE_LIMIT)
}

/// Insert one event directly into the store, returning the new `latest_ingest_seq`.
fn seed(k: &Kernel, id_byte: u8, ts: u64) -> u64 {
    k.event_store_handle()
        .insert(VerifiedEvent::from_raw_unchecked(raw(id_byte, ts)), &"wss://t/".to_string(), 0)
        .unwrap();
    k.event_store_handle().latest_ingest_seq().unwrap()
}

fn limits() -> PullLimits {
    PullLimits {
        max_entries: NonZeroUsize::new(64).unwrap(),
        max_scan_entries: NonZeroUsize::new(256).unwrap(),
    }
}

fn register(k: &mut Kernel, id: u64, after_seq: u64) {
    let handle = PullCursorHandle::from_raw(id);
    let spec = PullCursorSpec {
        consumer_id: PullConsumerId(format!("consumer-{id}")),
        scope: PullScope::GlobalLog,
        mode: PullCursorMode::GapAllowed,
        after_seq,
        limits: limits(),
    };
    k.open_pull_cursor(handle, spec);
}

fn pull_wake(k: &Kernel, id: u64) -> Option<u64> {
    k.store_wakeups.pull.get(&PullCursorId(id)).copied()
}

fn registry_len(k: &Kernel) -> usize {
    k.pull_cursor_registry.read().unwrap().len()
}

fn registry_has(k: &Kernel, id: u64) -> bool {
    k.pull_cursor_registry.read().unwrap().get(&PullCursorId(id)).is_some()
}

// ─── 1. Register emits initial wake when behind ──────────────────────────────

#[test]
fn register_emits_initial_wake_when_behind() {
    let mut k = new_kernel();
    let latest = seed(&k, 1, 1000);
    seed(&k, 2, 2000);
    let latest = k.event_store_handle().latest_ingest_seq().unwrap().max(latest);

    register(&mut k, 1, 0);

    assert_eq!(
        pull_wake(&k, 1),
        Some(latest),
        "register at after_seq=0 behind head must arm a wake at latest_seq"
    );
}

#[test]
fn register_no_wake_when_caught_up() {
    let mut k = new_kernel();
    let latest = seed(&k, 1, 1000);
    register(&mut k, 1, latest); // already at head
    assert_eq!(pull_wake(&k, 1), None, "a caught-up register must not arm a wake");
}

#[test]
fn register_cursor_id_zero_is_ignored() {
    let mut k = new_kernel();
    seed(&k, 1, 1000);
    register(&mut k, 0, 0);
    assert_eq!(registry_len(&k), 0, "cursor_id 0 is invalid and must not register");
    assert!(k.store_wakeups.pull.is_empty(), "cursor_id 0 must not arm a wake");
}

// ─── 2. Advance re-wakes when still behind ───────────────────────────────────

#[test]
fn advance_rewakes_when_still_behind() {
    let mut k = new_kernel();
    seed(&k, 1, 1000);
    seed(&k, 2, 2000);
    let latest = seed(&k, 3, 3000); // latest == 3

    register(&mut k, 1, 0);
    // Drain the initial wake — level-triggered re-arm keeps it (still behind).
    let drained = k.drain_pull_wakes();
    assert_eq!(drained, vec![(PullCursorId(1), latest)]);
    assert_eq!(
        pull_wake(&k, 1),
        Some(latest),
        "level-triggered: a cursor still behind is re-armed after drain"
    );

    // Consumer partially advances (still behind head) → re-wakes.
    k.advance_pull_cursor(PullCursorId(1), 1);
    assert_eq!(
        pull_wake(&k, 1),
        Some(latest),
        "advance to a seq still behind head must re-arm a wake"
    );
}

// ─── 3. Advance: no re-wake once caught up ───────────────────────────────────

#[test]
fn advance_no_wake_when_caught_up() {
    let mut k = new_kernel();
    seed(&k, 1, 1000);
    seed(&k, 2, 2000);
    let latest = seed(&k, 3, 3000);

    register(&mut k, 1, 0);
    // First drain: re-arms (still behind).
    let _ = k.drain_pull_wakes();
    assert_eq!(pull_wake(&k, 1), Some(latest));

    // Consumer advances to the head.
    k.advance_pull_cursor(PullCursorId(1), latest);
    // Drain again: the pending wake flushes, and the level-triggered re-arm now
    // finds the cursor caught up → nothing re-armed.
    let _ = k.drain_pull_wakes();
    assert_eq!(
        pull_wake(&k, 1),
        None,
        "a caught-up cursor must not be re-armed; the level trigger stops"
    );
}

/// Regression: advancing to head must CLEAR the pending wake immediately, so the
/// next drain emits NO stale duplicate. Before the fix, `advance` left the entry
/// that a prior drain re-armed, and the next drain emitted one ghost wake.
#[test]
fn advance_to_head_clears_wake_no_stale_duplicate_drain() {
    let mut k = new_kernel();
    seed(&k, 1, 1000);
    let latest = seed(&k, 2, 2000);

    register(&mut k, 1, 0);
    // First drain emits the wake and re-arms (cursor still behind head).
    let first = k.drain_pull_wakes();
    assert_eq!(first, vec![(PullCursorId(1), latest)], "first drain emits the wake");
    assert_eq!(pull_wake(&k, 1), Some(latest), "re-armed while still behind");

    // Consumer advances to head: the pending wake must be cleared right away.
    k.advance_pull_cursor(PullCursorId(1), latest);
    assert_eq!(
        pull_wake(&k, 1),
        None,
        "advance-to-head must clear the pending wake immediately (no-double-count)"
    );
    // The next drain must therefore be empty — no ghost/duplicate wake.
    assert!(
        k.drain_pull_wakes().is_empty(),
        "a caught-up cursor must not produce a stale duplicate wake on the next drain"
    );
}

// ─── 4. Unregister removes pending wake ──────────────────────────────────────

#[test]
fn unregister_removes_pending_wake() {
    let mut k = new_kernel();
    let latest = seed(&k, 1, 1000);
    register(&mut k, 1, 0);
    assert_eq!(pull_wake(&k, 1), Some(latest));
    assert!(registry_has(&k, 1));

    k.unregister_pull_cursor(PullCursorId(1));
    assert!(!registry_has(&k, 1), "unregister must drop the registry row");
    assert_eq!(pull_wake(&k, 1), None, "unregister must drop the pending wake");
}

// ─── 5. Coalescing: many appends → one latest_seq per cursor ─────────────────

#[test]
fn coalescing_many_appends_one_latest_seq_per_cursor() {
    let mut k = new_kernel();
    register(&mut k, 1, 0);

    // Five store mutations, each firing the chokepoint arm. The pull map must
    // coalesce to exactly one entry carrying the final latest_seq.
    let mut latest = 0u64;
    for i in 0..5u8 {
        latest = seed(&k, i, 1000 + i as u64);
        k.note_store_mutation(&hex64(i), &hex64(0xAA), 1, 1000 + i as u64, &[], true);
    }

    assert_eq!(k.store_wakeups.pull.len(), 1, "5 appends must coalesce to 1 wake entry");
    assert_eq!(pull_wake(&k, 1), Some(latest), "the coalesced wake carries the latest seq");
}

#[test]
fn note_store_mutation_arms_every_behind_cursor() {
    let mut k = new_kernel();
    register(&mut k, 1, 0);
    register(&mut k, 2, 0);
    let latest = seed(&k, 1, 1000);
    k.note_store_mutation(&hex64(1), &hex64(0xAA), 1, 1000, &[], true);

    assert_eq!(pull_wake(&k, 1), Some(latest));
    assert_eq!(pull_wake(&k, 2), Some(latest));
}

// ─── 6. MAX_PULL_CURSORS cap (loud no-op) ────────────────────────────────────

#[test]
fn max_pull_cursors_cap_is_loud_noop() {
    let mut k = new_kernel();
    // Fill to the cap (store empty → latest_seq 0 → after_seq 0 arms no wake).
    for id in 1..=MAX_PULL_CURSORS as u64 {
        register(&mut k, id, 0);
    }
    assert_eq!(registry_len(&k), MAX_PULL_CURSORS);

    // A NEW registration past the cap is a no-op — registry unchanged.
    let over = MAX_PULL_CURSORS as u64 + 1;
    register(&mut k, over, 0);
    assert_eq!(registry_len(&k), MAX_PULL_CURSORS, "new cursor past cap must not register");
    assert!(!registry_has(&k, over), "the over-cap cursor must be absent");

    // Replace-by-handle of an EXISTING cursor is always allowed (does not grow).
    k.open_pull_cursor(
        PullCursorHandle::from_raw(1),
        PullCursorSpec {
            consumer_id: PullConsumerId("replaced".to_string()),
            scope: PullScope::GlobalLog,
            mode: PullCursorMode::GapAllowed,
            after_seq: 0,
            limits: limits(),
        },
    );
    assert_eq!(registry_len(&k), MAX_PULL_CURSORS, "replace-by-id must not change the count");
    assert!(registry_has(&k, 1), "the replaced cursor must still be present");
}

// ─── 7. No timer/sleep/poll: wakes come only from StoreWakeups arms ──────────

#[test]
fn no_spontaneous_wake_without_an_arm() {
    let mut k = new_kernel();
    let latest = seed(&k, 1, 1000);

    // No cursors registered: a store mutation arms no pull wake.
    k.note_store_mutation(&hex64(1), &hex64(0xAA), 1, 1000, &[], true);
    assert!(k.store_wakeups.pull.is_empty(), "no cursor → no pull wake from a mutation");

    // A caught-up cursor: repeated drains never spontaneously produce a wake
    // (no timer/poll re-fires it). The only source is an arm.
    register(&mut k, 1, latest);
    for _ in 0..4 {
        assert!(k.drain_pull_wakes().is_empty(), "caught-up cursor must never self-wake");
        assert!(k.store_wakeups.pull.is_empty(), "no re-arm for a caught-up cursor");
    }
    assert!(!k.has_store_wakeups(), "no wake arm pending for a caught-up cursor");
}
