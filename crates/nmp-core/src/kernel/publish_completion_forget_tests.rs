//! D8 regression tests — `HandleCorrelationIndex` is forgotten on publish
//! COMPLETION (success and failure), not just on cancel/clear (S7, #1754).
//!
//! The index is bounded by [`super::handle_correlation::MAX_HANDLE_CORRELATION_ENTRIES`]
//! (D8), but that cap tracks the live in-flight set only when every terminal
//! path — success, failure, AND cancel — calls `forget`. Before the fix, the
//! engine-completion queue update never called `forget`, leaving a stale
//! handle↔correlation entry for every completed publish. (S11 slice 4 / #1758:
//! the queue update and that `forget` now live in the single engine-terminal
//! fold's settled arm — `apply_publish_queue_terminal`.) These tests prove the
//! fix: after a terminal engine completion the
//! index is empty (no stale entry survives). Split into its own file (not
//! appended to `cancel_correlation_tests.rs`) to stay within the 500-LOC
//! file-size baseline (AGENTS.md §file-size).

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_R1: &str = "wss://d8-forget-r1.test";
const WRITE_R2: &str = "wss://d8-forget-r2.test";

fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    kernel.seed_kind10002_for_test(author_pubkey, write_urls);
}

fn ok_payload<'a>(event_id: &'a str, accepted: bool, message: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message,
    }
}

/// Drive a relay through three transient NAKs until the engine gives up
/// (FailedAfterRetries after `transient_max_retries = 3` attempts).
fn drive_to_giveup(kernel: &mut Kernel, event_id: &str, relay: &str, base_ms: u64) {
    let _ = kernel.handle_publish_ok_at(
        relay,
        ok_payload(event_id, false, "io: down attempt 1"),
        base_ms + 100,
    );
    let _ = kernel.tick_publish_engine(base_ms + 1_500);
    let _ = kernel.handle_publish_ok_at(
        relay,
        ok_payload(event_id, false, "io: down attempt 2"),
        base_ms + 1_600,
    );
    let _ = kernel.tick_publish_engine(base_ms + 6_000);
    let _ = kernel.handle_publish_ok_at(
        relay,
        ok_payload(event_id, false, "io: down attempt 3"),
        base_ms + 6_100,
    );
}

/// Return the number of live entries in the kernel's handle↔correlation index.
/// This test module is a submodule of `kernel` so private fields are
/// accessible — no extra accessor on `Kernel` is needed (D8 introspection).
fn correlation_index_len(kernel: &Kernel) -> usize {
    kernel.publish_handle_correlation.len()
}

// ─── SUCCESS path ────────────────────────────────────────────────────────────

#[test]
fn handle_correlation_index_is_empty_after_successful_completion() {
    // D8 REGRESSION — PD-036 fix completeness. A publish that reaches the
    // SUCCESS terminal (all relays ACK) must remove its handle↔correlation
    // entry from the durable index. Without `publish_handle_correlation.forget`
    // in the engine-terminal fold's settled arm, the entry would survive
    // indefinitely (stale, bounded only by the cap, not the live in-flight set).
    //
    // This assertion FAILS against the pre-fix code (index still contains the
    // entry after completion) and PASSES after the fix.
    let author = "f1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e1".repeat(32).as_str(), &author, 1, "d8-success-forget");
    let correlation_id = "op-d8-success-1111".to_string();

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );
    // Index is populated while the publish is in flight.
    assert_eq!(
        correlation_index_len(&kernel),
        1,
        "handle↔correlation entry must be present while the publish is in flight"
    );

    // Drive both relays to OK — publish reaches the SUCCESS terminal.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    // The engine-terminal fold runs inside `handle_publish_ok_at` on the
    // second ack (the publish is now terminal). The index must be empty.
    assert_eq!(
        correlation_index_len(&kernel),
        0,
        "handle↔correlation index must be empty after a successful publish completion \
         (D8: the index tracks the live in-flight set, not all-time publishes)"
    );
}

#[test]
fn handle_correlation_index_is_empty_after_success_with_distinct_correlation_id() {
    // Same as the previous test but with an EXPLICIT distinct `correlation_id`
    // (the `PublishRaw` / registry-dispatched path). Both the handle↔correlation
    // AND the correlation↔handle self-mappings must be removed so `len()` is 0.
    let author = "f2".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e2".repeat(32).as_str(), &author, 1, "d8-raw-success");
    let minted_id = "d8dispatch11".repeat(4);
    assert_ne!(
        minted_id, signed.id,
        "fixture must use a correlation_id distinct from the event id"
    );

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(minted_id.clone()),
        0,
    );
    assert_eq!(correlation_index_len(&kernel), 1);

    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    assert_eq!(
        correlation_index_len(&kernel),
        0,
        "distinct-correlation_id publish: index must be empty after successful completion"
    );
}

// ─── FAILURE path ────────────────────────────────────────────────────────────

#[test]
fn handle_correlation_index_is_empty_after_all_relays_fail() {
    // D8 REGRESSION — failure terminal. A publish where all relays exhaust
    // their retry budget (FailedAfterRetries) must also forget the index entry.
    // Without the fix the entry survives indefinitely after the failed completion.
    let author = "f3".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e3".repeat(32).as_str(), &author, 1, "d8-failure-forget");
    let correlation_id = "op-d8-failure-2222".to_string();

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(correlation_id.clone()),
        0,
    );
    assert_eq!(
        correlation_index_len(&kernel),
        1,
        "entry must be present while in flight"
    );

    // Drive both relays to FailedAfterRetries — publish reaches FAILED terminal.
    drive_to_giveup(&mut kernel, &signed.id, WRITE_R1, 0);
    drive_to_giveup(&mut kernel, &signed.id, WRITE_R2, 100_000);

    assert_eq!(
        correlation_index_len(&kernel),
        0,
        "handle↔correlation index must be empty after a FAILED publish completion \
         (D8: stale entries must not survive terminal engine outcomes)"
    );
}

#[test]
fn handle_correlation_index_is_empty_after_permanent_rejection() {
    // A publish where all relays return a PERMANENT NIP-20 rejection
    // (`blocked:` prefix — no retry) must also forget the index entry.
    // The engine treats permanent rejections as immediate FailedAfterRetries.
    let author = "f4".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e4".repeat(32).as_str(), &author, 1, "d8-perm-reject");

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        None,
        0,
    );
    assert_eq!(correlation_index_len(&kernel), 1);

    // Both relays return a permanent rejection — engine terminates immediately.
    let _ = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        10,
    );
    let _ = kernel.handle_publish_ok_at(
        WRITE_R2,
        ok_payload(&signed.id, false, "blocked: spam"),
        20,
    );

    assert_eq!(
        correlation_index_len(&kernel),
        0,
        "index must be empty after a permanently-rejected publish"
    );
}

// ─── Non-terminal path (no forget) ───────────────────────────────────────────

#[test]
fn handle_correlation_index_is_non_empty_while_publish_is_in_flight() {
    // Guard: the index is NOT forgotten on a non-terminal intermediate ack.
    // Only one relay ACKs; the other is still in flight — the index entry must
    // remain so a cancel after the first ack still resolves correctly.
    let author = "f5".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e5".repeat(32).as_str(), &author, 1, "d8-inflight-guard");

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some("op-inflight-guard-3333".to_string()),
        0,
    );
    assert_eq!(correlation_index_len(&kernel), 1);

    // First relay ACKs — publish is NOT yet terminal (r2 still in flight).
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);

    // Index must still contain the entry — the publish is not terminal yet.
    assert_eq!(
        correlation_index_len(&kernel),
        1,
        "index must NOT be forgotten on a partial (non-terminal) ack \
         — only terminal outcomes (success/failure/cancel) forget the entry"
    );
}
