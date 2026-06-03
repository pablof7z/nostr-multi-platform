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

    k.claim_replaceable(0, PK, None, false);

    assert_eq!(
        k.pending_reverify_len(),
        0,
        "a still-fresh replaceable identity must NOT enqueue a reverify REQ",
    );
}

#[test]
fn force_enqueues_even_when_fresh() {
    // F-TTL — a forced claim bypasses the TTL gate: even a still-fresh identity
    // (check_again_after in the FUTURE) must enqueue a re-verification REQ.
    // This is the user-navigation / pull-to-refresh path that replaces the
    // removed `nmp_app_refresh_replaceable` FFI.
    let mut k = kernel_at(1_000_000);
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey: PK };
    k.event_store_handle().set_check_again_after(key, 2_000_000); // 1s in the future

    k.claim_replaceable(0, PK, None, true);

    assert_eq!(
        k.pending_reverify_len(),
        1,
        "force=true must enqueue a reverify REQ even when the identity is fresh",
    );
}

#[test]
fn expired_identity_enqueues_once() {
    // now = 2_000_000 ms; stamp check_again_after in the PAST → due.
    let mut k = kernel_at(2_000_000);
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey: PK };
    k.event_store_handle()
        .set_check_again_after(key, 1_000_000); // already elapsed

    k.claim_replaceable(0, PK, None, false);
    assert_eq!(
        k.pending_reverify_len(),
        1,
        "an expired replaceable identity must enqueue exactly one reverify REQ",
    );

    // In-flight guard: a second claim before EOSE must NOT double-enqueue,
    // because the first claim stamped check_again_after = now + INFLIGHT_GUARD_MS.
    k.claim_replaceable(0, PK, None, false);
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
    k.claim_replaceable(0, PK, None, false);
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
    k.claim_replaceable(30023, PK, Some("article-a".into()), false);
    k.claim_replaceable(30023, PK, Some("article-b".into()), false);
    assert_eq!(
        k.pending_reverify_len(),
        2,
        "two distinct d-tags on an addressable kind are two distinct identities",
    );
}

// ─── Blocker 3: ingest hook + EOSE handler stamp check_again_after ──────────

use super::nostr::NostrEvent;

/// Serialize a freshly-signed `nostr::Event` into the wire-shaped JSON that
/// `handle_event` deserializes (mirrors the `signed_note` fixture pattern, but
/// for arbitrary kinds/tags via `EventBuilder::new`).
fn signed_value(builder: ::nostr::EventBuilder, keys: &::nostr::Keys) -> serde_json::Value {
    let event = builder
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    serde_json::to_value(&event).expect("nostr::Event serializes to wire JSON")
}

#[test]
fn ingesting_kind0_stamps_check_again_after_with_ttl() {
    const NOW_MS: u64 = 1_700_000_000_000;
    let mut k = kernel_at(NOW_MS);

    let keys = ::nostr::Keys::generate();
    let pubkey = crate::kernel::hex_to_pubkey_bytes(&keys.public_key().to_hex())
        .expect("public key is 64-char hex");

    let value = signed_value(::nostr::EventBuilder::metadata(&::nostr::Metadata::new()), &keys);
    k.handle_event(RelayRole::Content, "wss://r.example/", "diag-firehose-stress", &value);

    // Default TTL for kind:0 is 1 hour (3_600_000 ms).
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey };
    assert_eq!(
        k.event_store_handle().get_check_again_after(&key),
        Some(NOW_MS + 3_600_000),
        "ingesting a kind:0 must stamp check_again_after = now + per-kind TTL",
    );
}

#[test]
fn ingesting_addressable_stamps_parameterized_key_with_d_tag() {
    const NOW_MS: u64 = 1_700_000_500_000;
    let mut k = kernel_at(NOW_MS);

    let keys = ::nostr::Keys::generate();
    let pubkey = crate::kernel::hex_to_pubkey_bytes(&keys.public_key().to_hex())
        .expect("public key is 64-char hex");

    let d_tag = "my-article";
    let builder = ::nostr::EventBuilder::new(::nostr::Kind::from(30023u16), "body")
        .tags([::nostr::Tag::parse(["d", d_tag]).expect("valid d tag")]);
    let value = signed_value(builder, &keys);
    k.handle_event(RelayRole::Content, "wss://r.example/", "diag-firehose-stress", &value);

    // The stamp must land on the PARAMETERIZED key carrying the d-tag — this is
    // the unique tag-extraction logic in the ingest hook. The default TTL
    // (6 hours) applies to kind:30023.
    let key = crate::store::ReplaceableKey::Parameterized {
        kind: 30023,
        pubkey,
        d_tag: d_tag.to_string(),
    };
    assert_eq!(
        k.event_store_handle().get_check_again_after(&key),
        Some(NOW_MS + 6 * 3_600_000),
        "ingesting an addressable event must stamp the d-tag-keyed identity",
    );

    // A different d-tag is a distinct identity and must NOT be stamped.
    let other = crate::store::ReplaceableKey::Parameterized {
        kind: 30023,
        pubkey,
        d_tag: "other".to_string(),
    };
    assert_eq!(
        k.event_store_handle().get_check_again_after(&other),
        None,
        "a different d-tag must be an independent (unstamped) identity",
    );
}

#[test]
fn eose_on_reverify_sub_stamps_tracked_keys_with_ttl() {
    const NOW_MS: u64 = 1_700_001_000_000;
    let mut k = kernel_at(NOW_MS);

    let pubkey = [9u8; 32];
    let key = crate::store::ReplaceableKey::Regular { kind: 0, pubkey };
    let sub_id = "reverify-0-0909090909090909-";

    // Seed the sub_id → key mapping the drain would record (the drain itself
    // needs configured outbox relays to emit a REQ; this isolates the EOSE
    // re-stamp arm from relay routing). Pre-stamp the in-flight guard value so
    // we can prove EOSE OVERWRITES it with the real per-kind TTL.
    k.event_store_handle()
        .set_check_again_after(key.clone(), NOW_MS + super::INFLIGHT_GUARD_MS);
    k.seed_reverify_sub_for_test(sub_id, vec![key.clone()]);

    // Drive the REAL EOSE path (`handle_text` with a genuine `["EOSE", sub_id]`
    // frame) — not a duplicated handler — so this exercises the wired code.
    let frame = serde_json::json!(["EOSE", sub_id]).to_string();
    let _ = k.handle_text(RelayRole::Indexer, "wss://r.example/", &frame);

    // After EOSE the identity is confirmed fresh: check_again_after =
    // now + per-kind TTL (1h for kind:0), replacing the larger in-flight guard.
    assert_eq!(
        k.event_store_handle().get_check_again_after(&key),
        Some(NOW_MS + 3_600_000),
        "EOSE on a reverify sub must re-stamp the tracked key with the per-kind TTL",
    );
    // And the sub is cleared from in-flight tracking.
    assert!(
        k.reverify_sub_ids_for_test().is_empty(),
        "EOSE must remove the reverify sub from in-flight tracking",
    );
}
