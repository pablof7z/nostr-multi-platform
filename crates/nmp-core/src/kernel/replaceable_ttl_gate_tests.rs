//! F-TTL gate tests — proof that `claim_replaceable` is TTL-gated.
//!
//! These tests exercise the central F-TTL invariant (Blocker 4): a claim only
//! enqueues a re-verification REQ when the cached identity's
//! `check_again_after` has elapsed. They run against `MemEventStore`, whose
//! `get/set_check_again_after` override now mirrors the LMDB backend, so the
//! gate logic is actually executed here (not bypassed by a no-op default).
//!
//! The clock is pinned with `FixedClock` so `now_ms()` is deterministic and we
//! can place the stored timestamp strictly in the past or the future relative
//! to "now".

use super::*;
use crate::kernel::clock::FixedClock;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Pin the kernel clock to a fixed wall-clock millisecond value.
fn kernel_at(now_ms: u64) -> Kernel {
    let mut k = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    k.set_clock(Arc::new(FixedClock(
        SystemTime::UNIX_EPOCH + Duration::from_millis(now_ms),
    )));
    k
}

const PK: [u8; 32] = [7u8; 32];

#[test]
fn fresh_identity_does_not_enqueue() {
    // now = 1_000_000 ms; stamp check_again_after in the FUTURE → still fresh.
    let mut k = kernel_at(1_000_000);
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey: PK };
    k.event_store_handle()
        .set_check_again_after(key, 2_000_000); // 1s in the future

    k.claim_replaceable(0, PK, None);

    assert_eq!(
        k.pending_reverify_len(),
        0,
        "a still-fresh replaceable identity must NOT enqueue a reverify REQ",
    );
}

#[test]
fn expired_identity_enqueues_once() {
    // now = 2_000_000 ms; stamp check_again_after in the PAST → due.
    let mut k = kernel_at(2_000_000);
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey: PK };
    k.event_store_handle()
        .set_check_again_after(key, 1_000_000); // already elapsed

    k.claim_replaceable(0, PK, None);
    assert_eq!(
        k.pending_reverify_len(),
        1,
        "an expired replaceable identity must enqueue exactly one reverify REQ",
    );

    // In-flight guard: a second claim before EOSE must NOT double-enqueue,
    // because the first claim stamped check_again_after = now + INFLIGHT_GUARD_MS.
    k.claim_replaceable(0, PK, None);
    assert_eq!(
        k.pending_reverify_len(),
        1,
        "the in-flight guard must prevent a duplicate enqueue before EOSE",
    );
}

#[test]
fn never_stamped_identity_is_due() {
    // No prior stamp → get_check_again_after returns None → treated as 0 → due.
    let mut k = kernel_at(5_000);
    k.claim_replaceable(0, PK, None);
    assert_eq!(
        k.pending_reverify_len(),
        1,
        "a cold (never-stamped) replaceable identity must re-verify eagerly",
    );
}

#[test]
fn addressable_claim_uses_parameterized_key() {
    // kind 30023 is addressable → the key must carry the d-tag so a distinct
    // d-tag is a distinct identity (independent gating).
    let mut k = kernel_at(10_000);
    k.claim_replaceable(30023, PK, Some("article-a".into()));
    k.claim_replaceable(30023, PK, Some("article-b".into()));
    assert_eq!(
        k.pending_reverify_len(),
        2,
        "two distinct d-tags on an addressable kind are two distinct identities",
    );
}
