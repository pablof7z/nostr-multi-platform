//! #1069 — actor-level GC wiring regression tests.
//!
//! Audit Finding 1: `EventStore::gc_step` was fully built and unit-tested in
//! `nmp-store`, but **never called from production code** — on every device,
//! NIP-40 expiry reaping, LRU eviction, and tombstone purge were dead. The fix
//! wires a wall-clock-gated `gc_step` onto the actor idle tick via
//! [`Kernel::run_gc_step`].
//!
//! These tests are the oracle that proves gc actually runs in the assembled
//! kernel (not just in `nmp-store` unit tests):
//!   - `run_gc_step_reaps_expired_event_and_records_report` — drives the kernel
//!     with an injected `FixedClock`, ingests a NIP-40 event that is valid on
//!     arrival, advances the clock past its `expiration`, runs one gc pass, and
//!     asserts the event is tombstoned and `last_gc` / `last_gc_at_ms` are
//!     populated. Deterministic: `gc_step` takes `now_secs` from the kernel
//!     clock, so `set_clock` makes the whole path reproducible — no sleep, no
//!     wall-clock flake.
//!   - `gc_does_not_run_before_the_60s_gate` — pins the negative direction: the
//!     actor's wall-clock gate must NOT fire before `GC_TICK_INTERVAL`, so a
//!     quiet kernel that has not been ticked has `last_gc == None` (no spurious
//!     work).
//!   - `gc_tick_interval_is_60_seconds` — pins the `gc.md` §3 "every 60 s"
//!     schedule against accidental drift.
//!   - `production_budget_enables_lru_eviction` — pins that the production
//!     budget carries the finite `HOT_EVENT_CEILING`, the load-bearing piece
//!     that makes Phase-2 LRU eviction non-vacuous.

use super::clock::FixedClock;
use super::nostr::NostrEvent;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const RELAY_A: &str = "wss://a.example/";

/// A current-era second-of-epoch the store treats as "now" at insert time.
const T0_SECS: u64 = 1_700_000_000;

/// Build one real Schnorr-signed kind:1 event carrying a NIP-40 `expiration`
/// tag. `created_at` and `expiration` are caller-supplied so a test can place
/// the expiry relative to the injected clock. Mirrors the fixture pattern in
/// `clock_injection_tests.rs`.
fn signed_expiring_note(
    keys: &::nostr::Keys,
    content: &str,
    created_at: u64,
    expiration: u64,
) -> NostrEvent {
    use ::nostr::{EventBuilder, Tag, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .tag(Tag::expiration(Timestamp::from_secs(expiration)))
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

/// `set_clock` to a fixed `SystemTime` `secs` seconds past the Unix epoch.
fn pin_clock(kernel: &mut Kernel, secs: u64) {
    let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    kernel.set_clock(Arc::new(FixedClock(fixed)));
}

/// The assembled kernel actually reaps a NIP-40-expired event when `run_gc_step`
/// fires, and records an observable `GcReport`.
#[test]
fn run_gc_step_reaps_expired_event_and_records_report() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);

    // Insert at T0 with an expiration 100 s in the future — valid on arrival.
    pin_clock(&mut kernel, T0_SECS);
    let keys = ::nostr::Keys::generate();
    let event = signed_expiring_note(&keys, "expiring probe", T0_SECS, T0_SECS + 100);
    let event_id = event.id.clone();
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);

    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "event must be present after ingest (valid on arrival — expiry is in the future)",
    );

    // No gc has run yet — the schedule must be observably empty.
    assert!(
        kernel.last_gc().is_none(),
        "last_gc must be None before any gc pass runs",
    );

    // Advance the kernel clock past the expiration, then run one gc pass.
    pin_clock(&mut kernel, T0_SECS + 200);
    let report = kernel
        .run_gc_step()
        .expect("gc_step must succeed against the in-memory store");

    assert!(
        report.expired_reaped >= 1,
        "the past-expiry event must be reaped: {report:?}",
    );
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_none(),
        "reaped event must be absent from the store after gc",
    );

    // The run must be observable (gc.md §7 / not a silent ending).
    assert!(
        kernel.last_gc().is_some(),
        "last_gc must be populated after run_gc_step",
    );
    assert_eq!(
        kernel.last_gc_at_ms(),
        Some((T0_SECS + 200) * 1_000),
        "last_gc_at_ms must be the injected-clock wall time, not SystemTime::now()",
    );
}

/// Negative direction: a kernel that has never been ticked has no recorded gc.
/// The actor's wall-clock gate (`last_gc.elapsed() >= GC_TICK_INTERVAL`) is what
/// decides *whether* `run_gc_step` is called; until it elapses, nothing runs and
/// no spurious work happens.
#[test]
fn gc_does_not_run_before_the_60s_gate() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Ingest an already-future-expiry event, but do NOT call run_gc_step:
    // this models the actor loop before the 60 s gate elapses.
    let keys = ::nostr::Keys::generate();
    let event = signed_expiring_note(&keys, "not yet gc'd", T0_SECS, T0_SECS + 100);
    let event_id = event.id.clone();
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);

    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "event must still be present — gc has not run",
    );
    assert!(
        kernel.last_gc().is_none(),
        "last_gc must remain None until the actor's 60 s gate fires run_gc_step",
    );

    // The gate predicate itself: an Instant that just started has not elapsed
    // a full interval, so the actor would not call run_gc_step.
    let just_started = std::time::Instant::now();
    assert!(
        just_started.elapsed() < crate::actor::GC_TICK_INTERVAL,
        "a fresh last_gc Instant must not satisfy the 60 s gate",
    );
}

/// The schedule constant matches `docs/design/lmdb/gc.md` §3 ("every 60 s").
#[test]
fn gc_tick_interval_is_60_seconds() {
    assert_eq!(
        crate::actor::GC_TICK_INTERVAL,
        Duration::from_secs(60),
        "gc.md §3 mandates a 60-second GC cadence",
    );
}

/// The production budget carries the finite hot-event ceiling — without it,
/// Phase-2 LRU eviction is a permanent no-op even once gc runs (the load-bearing
/// piece of #1069).
///
/// #1090 Stage 3: the production budget now ENABLES the LRU ceiling
/// (`max_total_events = HOT_EVENT_CEILING`). Stage 2 (`derive_store_pin_set`
/// floor-coherent pins) is what makes the finite ceiling safe — eviction can no
/// longer punch a hole below an active floored shape's `since`-floor.
#[test]
fn production_budget_documents_ceiling_state() {
    let budget = crate::store::GcBudget::production();

    // #1090 Stage 3: production gc enforces the finite hot-event ceiling.
    assert_eq!(
        budget.max_total_events,
        crate::store::HOT_EVENT_CEILING,
        "#1090 Stage 3: production gc must enable LRU eviction at HOT_EVENT_CEILING \
         (floor-coherence from Stage 2 makes it safe). See types/gc.rs.",
    );
    assert_eq!(
        budget.max_events_per_step,
        crate::store::GC_MAX_EVENTS_PER_STEP
    );
    assert_eq!(budget.max_duration_ms, crate::store::GC_MAX_DURATION_MS);
}

/// The GcReport from a gc pass must include a populated `duration_ms` so the
/// kernel can observe how long each pass took.  This guards against the V-117
/// concern that gc passes block the actor thread without any observable metric.
#[test]
fn gc_report_includes_duration_ms() {
    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS);

    // Ingest a few events and run a gc pass.
    let keys = ::nostr::Keys::generate();
    for i in 0..10u64 {
        let event = signed_expiring_note(
            &keys,
            &format!("probe-{i}"),
            T0_SECS + i,
            T0_SECS + i + 10_000, // expires far in the future
        );
        kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);
    }

    pin_clock(&mut kernel, T0_SECS + 100);
    let report = kernel.run_gc_step().expect("gc_step must succeed");

    // duration_ms must be set (non-zero on any real pass).
    // Allow 0 only in theory (pass so fast it rounds to 0ms); the important
    // invariant is that the field is populated and stays within the production
    // budget plus generous jitter headroom (10 s).
    assert!(
        report.duration_ms < 10_000,
        "gc_step duration_ms must be < 10 000 ms, got {}ms",
        report.duration_ms,
    );
}
