//! Deterministic clock-injection tests — proof that the `FixedClock` seam
//! (`kernel/clock.rs`, commit 204a0427) actually routes through the kernel
//! ingest path.
//!
//! `SystemTime::now()` reads that feed reducer output (`received_at_ms`
//! written into the `EventStore`) were extracted behind the `Clock` trait so
//! deterministic replay can substitute a fixed time. These tests are the
//! minimum viable proof for that: with a `FixedClock` installed, the stored
//! event's `received_at_ms` is bit-identical to the pinned clock value and
//! identical across multiple ingests — the actual replay-determinism
//! property, not just "the value happens to match a constant".
//!
//! Real Schnorr-signed events are used (`nostr::Keys::generate() +
//! EventBuilder::text_note + sign_with_keys`) — the `diag-firehose-` sub_id
//! bypasses the `timeline_authors` gate so any signed kind:1 reaches
//! `store.insert`. Same fixture pattern as `provenance_wire_tests.rs`; the
//! `signed_note` helper is duplicated rather than shared because this file's
//! concern (clock injection) is distinct from provenance counters.

use super::nostr::NostrEvent;
use super::*;
use crate::kernel::clock::FixedClock;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const RELAY_A: &str = "wss://a.example/";

/// Build one real Schnorr-signed kind:1 event using the supplied fixture
/// key. Returns the `NostrEvent` shape the kernel ingest path consumes after
/// JSON decoding (mirrors `provenance_wire_tests.rs::signed_note`).
///
/// `#[cfg(test)]`-only helper — `sign_with_keys` cannot fail with a
/// freshly-generated keypair; the `expect` is documentation, not a hot-path
/// concern.
fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

/// `received_at_ms` on the stored event is the injected `FixedClock` value,
/// not `SystemTime::now()`.
///
/// Negative case: with `set_clock` removed, the assertion compares a pinned
/// constant against a real wall-clock reading and fails loudly — the test
/// genuinely exercises the seam.
#[test]
fn received_at_ms_uses_injected_clock() {
    // Pin the clock to a distinctive current-era millisecond value. The
    // `.123` suffix rules out anyone "fixing" a future failure with the
    // `unwrap_or(0)` sentinel — a real-looking timestamp keeps the test
    // self-documenting.
    const FIXED_MS: u64 = 1_700_000_000_123;
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_millis(FIXED_MS);

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    kernel.set_clock(Arc::new(FixedClock(fixed)));

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "clock-injection probe", 1_700_000_000);
    let event_id = event.id.clone();

    // `diag-firehose-` sub_id bypasses the `timeline_authors` gate so the
    // signed kind:1 reaches `store.insert`, where `received_at_ms` is
    // stamped from `self.clock.now()` (ingest/timeline.rs).
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);

    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    let stored = kernel
        .store
        .get_by_id(&id_bytes)
        .expect("store get_by_id must not error")
        .expect("ingested event must be present in the store");

    assert_eq!(
        stored.received_at_ms, FIXED_MS,
        "received_at_ms must be the injected FixedClock value, not SystemTime::now()",
    );
}

/// The replay-determinism property: two ingests under the same `FixedClock`
/// produce bit-identical `received_at_ms`.
///
/// With the production `SystemClock` the two stamps would differ by
/// microseconds-to-milliseconds; with `FixedClock` they are equal. This is
/// the property deterministic replay actually depends on — a reducer run
/// twice over the same input emits the same timestamp output.
#[test]
fn injected_clock_makes_received_at_ms_deterministic_across_ingests() {
    const FIXED_MS: u64 = 1_700_000_042_999;
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_millis(FIXED_MS);

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    kernel.set_clock(Arc::new(FixedClock(fixed)));

    let keys = ::nostr::Keys::generate();
    let first = signed_note(&keys, "first", 1_700_000_001);
    let second = signed_note(&keys, "second", 1_700_000_002);
    let first_id = first.id.clone();
    let second_id = second.id.clone();

    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", first);
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", second);

    let read = |id_hex: &str| -> u64 {
        let id_bytes = crate::kernel::hex_to_pubkey_bytes(id_hex).expect("event id is 64-char hex");
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .expect("ingested event must be present in the store")
            .received_at_ms
    };

    let first_ms = read(&first_id);
    let second_ms = read(&second_id);

    assert_eq!(
        first_ms, FIXED_MS,
        "first ingest stamps the FixedClock value"
    );
    assert_eq!(
        first_ms, second_ms,
        "both ingests under the same FixedClock must stamp identical \
         received_at_ms — the deterministic-replay property",
    );
}

/// D9 (ADR-0070) — a relay-supplied event with a FUTURE `created_at` must be
/// clamped to the kernel's `now` on the OBSERVER-DELIVERED `KernelEvent`, so a
/// hostile/buggy relay cannot pin an event to the top of app feeds (which order
/// by `KernelEvent.created_at` — `nmp-feed` / `nmp-nip01::FlatFeed`). This is a
/// universal hostile-relay invariant applied once at the single chokepoint
/// observer fan-out, protecting ALL feed consumers. The timeline read-cache
/// (`self.events`, which backs the kernel's own timeline ordering) is also
/// clamped (strictly stronger). The authoritative `EventStore` row retains the
/// original wire timestamp for protocol correctness.
///
/// A past-dated event passes through unchanged — clamping is `min(wire, now)`,
/// never an unconditional overwrite.
#[test]
fn future_dated_event_created_at_clamped_to_now_on_observer_and_in_projection() {
    use crate::actor::{new_event_observer_slot, register_rust_observer, ObservedProjectionSink};
    use crate::substrate::KernelEvent;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Captures every fanned-out `KernelEvent` keyed by id so the test can read
    /// back the observer-visible `created_at`.
    struct CapturingObserver {
        seen: Mutex<HashMap<String, u64>>,
    }
    impl ObservedProjectionSink for CapturingObserver {
        fn on_kernel_event(&self, event: &KernelEvent) {
            self.seen
                .lock()
                .unwrap()
                .insert(event.id.clone(), event.created_at);
        }
    }

    // Pin the kernel clock to a known "now" = 1_000_000 secs.
    const NOW_SECS: u64 = 1_000_000;
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS);

    let slot = new_event_observer_slot();
    let observer = Arc::new(CapturingObserver {
        seen: Mutex::new(HashMap::new()),
    });
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    kernel.set_clock(Arc::new(FixedClock(fixed)));
    kernel.set_event_observers_handle(slot);

    // A future-dated event (now + 9999) and a past-dated event (now - 500_000),
    // each a real Schnorr-signed kind:1 so `ingest_timeline_event` accepts it.
    let keys = ::nostr::Keys::generate();
    let future = signed_note(&keys, "from the future", NOW_SECS + 9_999);
    let past = signed_note(&keys, "from the past", NOW_SECS - 500_000);
    let future_id = future.id.clone();
    let past_id = past.id.clone();

    // `diag-firehose-` sub_id bypasses the `timeline_authors` gate.
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", future);
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", past);

    // ADR-0070 — the timeline READ-CACHE projection (`self.events`, which backs
    // the timeline ordering) is clamped to `now` for the future-dated event so
    // it cannot pin to the top of the feed; the past-dated event passes through.
    assert_eq!(
        kernel.events.get(&future_id).map(|e| e.created_at),
        Some(NOW_SECS),
        "future-dated created_at must be clamped to now in the timeline read-cache projection (D9)"
    );
    assert_eq!(
        kernel.events.get(&past_id).map(|e| e.created_at),
        Some(NOW_SECS - 500_000),
        "past-dated created_at must pass through unchanged — clamp is min(wire, now)"
    );

    // ADR-0070 (D9 blocker fix) — the app-observer fan-out (the input to every
    // app feed) MUST also clamp the future-dated `created_at` to now, else a
    // hostile event sorts to the top of `nmp-feed` / `FlatFeed`. The past-dated
    // event passes through unchanged.
    let seen = observer.seen.lock().unwrap();
    assert_eq!(
        seen.get(&future_id).copied(),
        Some(NOW_SECS),
        "the chokepoint observer fan-out must clamp a future created_at to now (D9 — feeds order by it)"
    );
    assert_eq!(
        seen.get(&past_id).copied(),
        Some(NOW_SECS - 500_000),
        "a past-dated event passes through the observer fan-out unchanged (clamp is min(wire, now))"
    );

    // The authoritative store row retains the ORIGINAL wire timestamp for
    // protocol correctness; only the timeline read-cache projection is clamped.
    let future_bytes =
        crate::kernel::hex_to_pubkey_bytes(&future_id).expect("event id is 64-char hex");
    let stored = kernel
        .store
        .get_by_id(&future_bytes)
        .expect("store get_by_id must not error")
        .expect("future event ingested");
    assert_eq!(
        stored.raw.created_at,
        NOW_SECS + 9_999,
        "StoredEvent must retain the unclamped wire created_at for protocol correctness"
    );
}
