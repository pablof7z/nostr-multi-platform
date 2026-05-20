//! Clock-discipline tests — proof that the kernel rejects relay-supplied
//! events whose `created_at` is unreasonably far in the future.
//!
//! ## Threat model
//!
//! Nostr relay operators (or a malicious relay) can deliver events with an
//! arbitrary `created_at` — the Schnorr signature covers `created_at`, but a
//! signer can sign *any* timestamp, so the signature does NOT bound it. For
//! replaceable kinds (0, 3, 10002) the store picks the canonical "winner" by
//! strict `>` on `created_at`. A future-dated event would therefore
//! permanently displace a legitimate newer one and resist replacement.
//!
//! `handle_event` (`ingest/mod.rs`) enforces a `MAX_FUTURE_SECONDS` (15 min)
//! clock-skew tolerance: events with `created_at > now() + 900s` are dropped
//! before any counter bump, raw-tap, store insert, or per-kind dispatch. The
//! check reads the injected `Clock` so it is deterministic under `FixedClock`.
//!
//! Scope: this addresses *future*-dating only. Past-dating is a separate
//! concern (you cannot reject all past events — backfill is legitimate).
//!
//! Fixture pattern mirrors `clock_injection_tests.rs`: real Schnorr-signed
//! events via `EventBuilder` + `sign_with_keys`, ingested through the public
//! `handle_event` path, with a pinned `FixedClock`.

use super::*;
use crate::kernel::clock::FixedClock;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const RELAY_A: &str = "wss://a.example/";

/// Pinned "now" for the fixed clock — a real-looking current-era unix second.
const NOW_SECS: u64 = 1_700_000_000;

/// A real Schnorr-signed kind:10002 (NIP-65 relay list) event with a
/// caller-chosen `created_at`, returned as the `serde_json::Value` the kernel
/// decodes off the wire (`handle_event`'s third argument).
///
/// kind:10002 is replaceable, so a future-dated instance is exactly the
/// attack this doctrine defends against. `created_at` is also returned
/// separately because the signed event's hex id is needed to look the row up
/// in the store afterwards.
///
/// `#[cfg(test)]`-only helper — `sign_with_keys` cannot fail with a
/// freshly-generated keypair.
fn signed_relay_list(keys: &::nostr::Keys, created_at: u64) -> (serde_json::Value, String) {
    use ::nostr::util::JsonUtil as _;
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let nostr_event = EventBuilder::new(Kind::RelayList, "")
        .tags([Tag::parse(["r", "wss://relay.example/"]).expect("valid r tag")])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let id = nostr_event.id.to_hex();
    // `nostr::Event` serializes to the NIP-01 canonical event object — the
    // exact wire shape `handle_event` deserializes into `NostrEvent`.
    let value: serde_json::Value =
        serde_json::from_str(&nostr_event.as_json()).expect("nostr::Event emits valid JSON");
    (value, id)
}

/// Build a kernel with the clock pinned to `NOW_SECS`.
fn kernel_at_now() -> Kernel {
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS);
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    kernel.set_clock(Arc::new(FixedClock(fixed)));
    kernel
}

/// `true` iff the event with `id_hex` is present in the store.
fn stored(kernel: &Kernel, id_hex: &str) -> bool {
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(id_hex).expect("event id is 64-char hex");
    kernel
        .store
        .get_by_id(&id_bytes)
        .expect("store get_by_id must not error")
        .is_some()
}

/// An event 16 minutes in the future (960s, past the 900s threshold) is
/// dropped before reaching the store.
#[test]
fn future_dated_event_beyond_threshold_is_rejected() {
    let mut kernel = kernel_at_now();
    let keys = ::nostr::Keys::generate();
    // 16 minutes = 960s > MAX_FUTURE_SECONDS (900s).
    let (event, event_id) = signed_relay_list(&keys, NOW_SECS + 16 * 60);

    kernel.handle_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", &event);

    assert!(
        !stored(&kernel, &event_id),
        "an event 16 minutes in the future must be dropped (clock discipline)",
    );
}

/// An event 14 minutes in the future (840s, inside the 900s threshold) is
/// accepted — relays/signers legitimately drift by minutes.
#[test]
fn future_dated_event_within_threshold_is_accepted() {
    let mut kernel = kernel_at_now();
    let keys = ::nostr::Keys::generate();
    // 14 minutes = 840s < MAX_FUTURE_SECONDS (900s).
    let (event, event_id) = signed_relay_list(&keys, NOW_SECS + 14 * 60);

    kernel.handle_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", &event);

    assert!(
        stored(&kernel, &event_id),
        "an event 14 minutes in the future is inside clock-skew tolerance and must be accepted",
    );
}

/// The boundary: exactly `now + MAX_FUTURE_SECONDS` (900s) is accepted — the
/// check is strict `>`, so the threshold value itself is not future-dated.
#[test]
fn future_dated_event_exactly_at_threshold_is_accepted() {
    let mut kernel = kernel_at_now();
    let keys = ::nostr::Keys::generate();
    let (event, event_id) = signed_relay_list(&keys, NOW_SECS + 900);

    kernel.handle_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", &event);

    assert!(
        stored(&kernel, &event_id),
        "created_at exactly at now + 900s is the boundary and must be accepted (strict `>`)",
    );
}

/// One second past the boundary (`now + 901s`) is rejected — off-by-one guard.
#[test]
fn future_dated_event_one_second_past_threshold_is_rejected() {
    let mut kernel = kernel_at_now();
    let keys = ::nostr::Keys::generate();
    let (event, event_id) = signed_relay_list(&keys, NOW_SECS + 901);

    kernel.handle_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", &event);

    assert!(
        !stored(&kernel, &event_id),
        "created_at at now + 901s is one second past tolerance and must be dropped",
    );
}

/// A past-dated event is always accepted — backfill is legitimate, and this
/// doctrine deliberately only bounds the *future* direction.
#[test]
fn past_dated_event_is_accepted() {
    let mut kernel = kernel_at_now();
    let keys = ::nostr::Keys::generate();
    // One hour in the past.
    let (event, event_id) = signed_relay_list(&keys, NOW_SECS - 3600);

    kernel.handle_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", &event);

    assert!(
        stored(&kernel, &event_id),
        "a past-dated event is legitimate backfill and must be accepted",
    );
}
