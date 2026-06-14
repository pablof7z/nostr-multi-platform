//! K3 Stage D1 — kernel-side coverage-ledger WRITE-path tests (ADR-0056 §3).
//!
//! These tests drive the production `handle_message`/`handle_text` EOSE seam
//! (the same surface `eose_ok_notice_ingest_tests` uses) and the
//! `record_neg_done_coverage` NEG-DONE entry point, and assert what lands in the
//! coverage ledger via `EventStore::get_coverage`. They lock the four D1
//! contracts:
//!
//! 1. flag ON + un-floored EOSE ⇒ a row advances to `now` for `(hash, relay)`;
//! 2. flag OFF ⇒ NO row is ever written (the default, zero-behavior-change case);
//! 3. a `since`-floored EOSE is NOT over-claimed (no `[0, now]` row);
//! 4. NEG-DONE advances the ledger to `now` (un-floored full-window per Stage A).
//!
//! The since-floor READ path (`apply_watermark_rewrite`) is deliberately NOT
//! touched in D1 and is not exercised here — D2 swaps the read.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use crate::kernel::clock::FixedClock;
use crate::kernel::Kernel;
use crate::kernel::RelayFrame;
use crate::planner::{InterestId, InterestLifecycle};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::subs::WireFrame;

const RELAY: &str = "wss://relay.coverage-test";
// A planner-shaped wire id: `sub-<canonical_filter_hash>`. The ledger key half
// is the hash after the `sub-` prefix — read back by exactly that value.
const SUB_ID: &str = "sub-deadbeefdeadbeef";
const FILTER_HASH: &str = "deadbeefdeadbeef";
// Pin the clock so `now_secs()` is deterministic.
const NOW_SECS: u64 = 1_700_000_000;

fn kernel_at(now_secs: u64) -> Kernel {
    let mut k = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    k.set_clock(Arc::new(FixedClock(
        UNIX_EPOCH + Duration::from_secs(now_secs),
    )));
    k
}

/// Read the coverage watermark for `(FILTER_HASH, RELAY)` from the kernel store.
fn coverage(kernel: &Kernel) -> Option<u64> {
    kernel.event_store_handle().get_coverage(FILTER_HASH, RELAY)
}

/// Register a wire sub through the production planner path so `since_floor` is
/// parsed from the filter exactly as in production.
fn register_req(kernel: &mut Kernel, filter_json: &str, lifecycle: InterestLifecycle) {
    kernel.register_wire_frames_for_test(&[WireFrame::Req {
        relay_url: RELAY.to_string(),
        sub_id: SUB_ID.to_string(),
        filter_json: filter_json.to_string(),
        interest_id: InterestId(42),
        lifecycle,
    }]);
}

fn deliver_eose(kernel: &mut Kernel) {
    let eose = serde_json::json!(["EOSE", SUB_ID]).to_string();
    kernel.handle_message(RelayRole::Content, RELAY, RelayFrame::Text(eose));
}

// ─── 1. flag ON + un-floored EOSE advances the ledger ──────────────────────────

#[test]
fn eose_on_unfloored_req_advances_ledger_to_now_when_flag_on() {
    let mut kernel = kernel_at(NOW_SECS);
    kernel.set_coverage_ledger_enabled(true);

    // Un-floored REQ (no `since`): an EOSE honestly proves `[0, now]`.
    register_req(
        &mut kernel,
        r#"{"kinds":[1],"authors":["aa"]}"#,
        InterestLifecycle::OneShot,
    );
    assert_eq!(coverage(&kernel), None, "precondition: ledger empty");

    deliver_eose(&mut kernel);

    assert_eq!(
        coverage(&kernel),
        Some(NOW_SECS),
        "an un-floored EOSE with the flag ON must advance coverage to now",
    );
}

// ─── 2. flag OFF writes nothing (the default, zero-behavior-change case) ────────

#[test]
fn eose_writes_nothing_when_flag_off() {
    // Default kernel: coverage_ledger_enabled is false.
    let mut kernel = kernel_at(NOW_SECS);
    assert!(
        !kernel.coverage_ledger_enabled(),
        "the coverage ledger must be OFF by default — D1 ships dormant",
    );

    register_req(
        &mut kernel,
        r#"{"kinds":[1],"authors":["aa"]}"#,
        InterestLifecycle::OneShot,
    );
    deliver_eose(&mut kernel);

    assert_eq!(
        coverage(&kernel),
        None,
        "with the flag OFF, an EOSE must write zero coverage rows",
    );
}

// ─── 3. a since-floored EOSE is NOT over-claimed ───────────────────────────────

#[test]
fn eose_on_floored_req_does_not_overclaim_below_floor() {
    let mut kernel = kernel_at(NOW_SECS);
    kernel.set_coverage_ledger_enabled(true);

    // A `since`-floored REQ: the EOSE proves only `[floor, now]`, NOT `[0, now]`.
    // The downward-closed ledger must therefore NOT advance — recording `now`
    // would over-claim `[0, floor)`, the very unsoundness ADR-0056 §1 names.
    register_req(
        &mut kernel,
        r#"{"kinds":[1],"authors":["aa"],"since":1500000000}"#,
        InterestLifecycle::Tailing,
    );
    deliver_eose(&mut kernel);

    assert_eq!(
        coverage(&kernel),
        None,
        "a since-floored EOSE must not write a [0, now] coverage row \
         (no over-claim below the floor)",
    );
}

// ─── 4. NEG-DONE advances the ledger (un-floored full window per Stage A) ───────

#[test]
fn neg_done_advances_ledger_to_now_when_flag_on() {
    let mut kernel = kernel_at(NOW_SECS);
    kernel.set_coverage_ledger_enabled(true);

    // The NIP-77 runtime calls this at the terminal `Done` outcome. Per Stage A
    // the NEG window is un-floored `[0, ∞)`, so a completed reconciliation
    // honestly covers `[0, now]`.
    kernel.record_neg_done_coverage(SUB_ID, RELAY, kernel.now_secs());

    assert_eq!(
        coverage(&kernel),
        Some(NOW_SECS),
        "NEG-DONE with the flag ON must advance coverage to now",
    );
}

#[test]
fn neg_done_writes_nothing_when_flag_off() {
    let kernel = kernel_at(NOW_SECS);
    // Flag off by default.
    kernel.record_neg_done_coverage(SUB_ID, RELAY, kernel.now_secs());
    assert_eq!(
        coverage(&kernel),
        None,
        "with the flag OFF, NEG-DONE must write zero coverage rows",
    );
}

// ─── 5. the key is the canonical filter hash (D2 reads by the same key) ─────────

#[test]
fn coverage_is_keyed_by_canonical_filter_hash_not_full_sub_id() {
    let mut kernel = kernel_at(NOW_SECS);
    kernel.set_coverage_ledger_enabled(true);
    register_req(
        &mut kernel,
        r#"{"kinds":[1],"authors":["aa"]}"#,
        InterestLifecycle::OneShot,
    );
    deliver_eose(&mut kernel);

    // Recorded under the hash (after the `sub-` prefix), which is the key
    // `recompile` will read by in D2 — NOT under the full `sub-<hash>` id.
    assert_eq!(
        kernel.event_store_handle().get_coverage(FILTER_HASH, RELAY),
        Some(NOW_SECS),
    );
    assert_eq!(
        kernel.event_store_handle().get_coverage(SUB_ID, RELAY),
        None,
        "coverage must be keyed by the canonical filter hash, not the full sub_id",
    );
}
