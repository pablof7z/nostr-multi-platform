//! T128 integration tests — `PublishQueueEntry` terminal status transitions.
//!
//! T117 wired the kernel's publish path through `PublishEngine` but kept the
//! `PublishQueueEntry.status` pinned at `"accepted_locally"` so the iOS Pulse
//! `ComposeView` wouldn't break. T128 lifts that pin: the engine's terminal
//! verdict (Ok / FailedAfterRetries per-relay, settled when every relay has
//! reached a terminal state) now flips the queue entry to `"ok"` / `"failed"`
//! and carries a per-relay outcome map for the UI.
//!
//! These tests pin the *queue-entry* contract — they snapshot
//! `Kernel::publish_queue_snapshot()` after the relevant engine drive and
//! assert on `status` + `relay_outcomes`. The engine-snapshot side
//! (`recent_ok`, `recent_errors`) is already covered by
//! `publish_engine_tests.rs`; the two contracts are complementary, not
//! redundant. New file (not appended to `publish_engine_tests.rs`) because
//! that file is already 476 LOC and adding ~200 more would breach the 500 LOC
//! hard cap.
//!
//! T-publish-resolver-indexer (codex f81f735): tests updated to seed
//! kind:10002 for each author so `Nip65OutboxResolver` routes via NIP-65
//! rather than the now-removed indexer fallback.

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};

/// T128 test relay URLs — declared as NIP-65 write relays in kind:10002.
const WRITE_R1: &str = "wss://t128-write-r1.test";
const WRITE_R2: &str = "wss://t128-write-r2.test";

fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn ok_payload<'a>(event_id: &'a str, accepted: bool, reason: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

/// Seed a kind:10002 into the kernel's event store for `author_pubkey` with
/// `write_urls` as write-marker relay tags. Required by T-publish-resolver-
/// indexer: without a kind:10002 the resolver returns empty (NoTargets).
fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    let tags: Vec<Vec<String>> = write_urls
        .iter()
        .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
        .collect();
    let id_prefix = &author_pubkey[..2];
    let id = format!("{:0<64}", format!("{}k10002", id_prefix));
    let raw = RawEvent {
        id,
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel
        .store
        .insert(verified, &"wss://seed".to_string(), 1_700_000_000_000)
        .expect("seed_kind10002 insert");
}

/// Helper: locate the queue entry for `event_id` in the kernel's snapshot.
/// Panics if missing — every T128 test pushes one entry before asserting.
fn entry_for<'a>(
    kernel: &'a Kernel,
    event_id: &str,
) -> &'a crate::kernel::PublishQueueEntry {
    kernel
        .publish_queue_snapshot()
        .iter()
        .find(|e| e.event_id == event_id)
        .expect("queue entry must exist for the publish under test")
}

#[test]
fn t128_all_relays_ack_flips_status_to_ok_with_full_outcome_map() {
    // Happy path: both NIP-65 write relays land OK acks → engine settles the
    // publish terminally → `apply_engine_completions` flips the queue
    // entry's `status` from `accepted_locally` to `"ok"` and fills
    // `relay_outcomes` with one `"ok"` row per relay.
    let author = "22".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "11".repeat(32).as_str(),
        &author,
        1,
        "all-ack t128",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 1_000);
    assert_eq!(outbound.len(), 2, "two NIP-65 write relays expected");

    // Immediately after `run_publish_engine_at` (no acks yet) the entry
    // sits at `accepted_locally` with an empty outcome map.
    {
        let entry = entry_for(&kernel, &signed.id);
        assert_eq!(entry.status, "accepted_locally");
        assert!(
            entry.relay_outcomes.is_empty(),
            "no per-relay verdicts before any ack arrives"
        );
        assert_eq!(entry.target_relays, 2);
    }

    // First ack — publish is NOT yet terminal (one relay still in-flight),
    // so the entry must stay at `accepted_locally`.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 1_010);
    {
        let entry = entry_for(&kernel, &signed.id);
        assert_eq!(
            entry.status, "accepted_locally",
            "partial-progress acks must not promote the entry past accepted_locally"
        );
        assert!(
            entry.relay_outcomes.is_empty(),
            "per-relay outcomes surface only on terminal verdict"
        );
    }

    // Second ack — every relay has now settled → engine drains a
    // `TerminalOutcome` into `recently_completed` → `apply_engine_completions`
    // applies it → queue entry flips to `"ok"`.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 1_020);
    let entry = entry_for(&kernel, &signed.id);
    assert_eq!(entry.status, "ok", "all-ACK publish settles as ok");
    assert_eq!(
        entry.relay_outcomes.len(),
        2,
        "every relay must appear in the outcome map"
    );
    for outcome in &entry.relay_outcomes {
        assert_eq!(outcome.status, "ok", "every per-relay outcome is ok");
        assert!(outcome.message.is_empty(), "no message on an ok outcome");
        assert!(
            outcome.relay_url == WRITE_R1 || outcome.relay_url == WRITE_R2,
            "outcome relay_url must be one of the declared write relays; got {}",
            outcome.relay_url
        );
    }
    // No duplicates — the engine reports each relay exactly once.
    let urls: std::collections::BTreeSet<String> = entry
        .relay_outcomes
        .iter()
        .map(|o| o.relay_url.clone())
        .collect();
    assert_eq!(urls.len(), 2, "outcome map must have no duplicate relays");
}

#[test]
fn t128_all_relays_give_up_flips_status_to_failed_with_failure_reasons() {
    // Pure failure path: r1 and r2 both keep returning transient io errors
    // until the engine gives up (transient_max_retries = 3 by default).
    // After give-up the queue entry must read `"failed"` with both relays
    // listed under `relay_outcomes` carrying the give-up reason.
    let author = "44".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "33".repeat(32).as_str(),
        &author,
        1,
        "all-fail t128",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);
    assert_eq!(outbound.len(), 2);

    // Helper closure: drive a single relay through three transient acks +
    // two ticks (the third ack triggers FailedAfterRetries). Uses unique
    // timestamps so `apply_ack`'s late-ack idempotence path doesn't drop us.
    let drive_to_giveup = |kernel: &mut Kernel, relay: &str, base_ms: u64| {
        // Attempt 1 → schedule retry at base + 1_000.
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 1"),
            base_ms + 100,
        );
        // Tick past the 1s backoff → dispatch attempt 2.
        let _ = kernel.tick_publish_engine(base_ms + 1_500);
        // Attempt 2 → schedule retry at +4_000.
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 2"),
            base_ms + 1_600,
        );
        // Tick past the 4s backoff → dispatch attempt 3.
        let _ = kernel.tick_publish_engine(base_ms + 6_000);
        // Attempt 3 → engine gives up (FailedAfterRetries).
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 3"),
            base_ms + 6_100,
        );
    };

    // Base offsets so r2's give-up `now_ms` is strictly past r1's last
    // recorded timestamp — apply_ack's "stale duplicate" branch is keyed on
    // per-relay state, not global clock, but distinct timestamps make the
    // test's intent obvious.
    drive_to_giveup(&mut kernel, WRITE_R1, 0);
    drive_to_giveup(&mut kernel, WRITE_R2, 100_000);

    let entry = entry_for(&kernel, &signed.id);
    assert_eq!(
        entry.status, "failed",
        "all-fail publish must settle as failed; got status {} outcomes={:?}",
        entry.status, entry.relay_outcomes
    );
    assert_eq!(
        entry.relay_outcomes.len(),
        2,
        "every relay's give-up must surface in the outcome map"
    );
    for outcome in &entry.relay_outcomes {
        assert_eq!(
            outcome.status, "failed",
            "every per-relay outcome must be failed on the all-fail path"
        );
        assert!(
            outcome.message.contains("transient"),
            "give-up reason should be transient-flavoured: {}",
            outcome.message
        );
    }
}

#[test]
fn t128_partial_success_reports_ok_with_mixed_outcome_map() {
    // Mixed path: r1 acks OK, r2 burns through all retries and fails.
    // Per the iOS UX requirement the queue entry reports `"ok"` (the publish
    // landed on at least one relay) and the outcome map carries both verdicts
    // so the ComposeView can render "Published to 1/2 relays".
    let author = "66".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "55".repeat(32).as_str(),
        &author,
        1,
        "partial t128",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);
    assert_eq!(outbound.len(), 2);

    // r1 settles OK on attempt 1.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);

    // Entry stays at `accepted_locally` — r2 is still in flight.
    assert_eq!(entry_for(&kernel, &signed.id).status, "accepted_locally");

    // r2 burns through three transient attempts.
    let _ = kernel.handle_publish_ok_at(
        WRITE_R2,
        ok_payload(&signed.id, false, "io: down 1"),
        100,
    );
    let _ = kernel.tick_publish_engine(1_500);
    let _ = kernel.handle_publish_ok_at(
        WRITE_R2,
        ok_payload(&signed.id, false, "io: down 2"),
        1_600,
    );
    let _ = kernel.tick_publish_engine(6_000);
    let _ = kernel.handle_publish_ok_at(
        WRITE_R2,
        ok_payload(&signed.id, false, "io: down 3"),
        6_100,
    );

    let entry = entry_for(&kernel, &signed.id);
    assert_eq!(
        entry.status, "ok",
        "partial-success publish reports ok (at least one relay accepted)"
    );
    assert_eq!(entry.relay_outcomes.len(), 2);
    let r1_outcome = entry
        .relay_outcomes
        .iter()
        .find(|o| o.relay_url == WRITE_R1)
        .expect("r1 outcome must be present");
    let r2_outcome = entry
        .relay_outcomes
        .iter()
        .find(|o| o.relay_url == WRITE_R2)
        .expect("r2 outcome must be present");
    assert_eq!(r1_outcome.status, "ok");
    assert!(r1_outcome.message.is_empty());
    assert_eq!(r2_outcome.status, "failed");
    assert!(
        r2_outcome.message.contains("transient"),
        "failed outcome must carry the give-up reason: {}",
        r2_outcome.message
    );
}

#[test]
fn t128_late_ack_after_terminal_does_not_re_flip_status() {
    // Idempotence contract: once the queue entry has flipped to `"ok"`, a
    // late-arriving ack (e.g. a slow relay re-sending OK for the same event
    // post-settlement) must not perturb the terminal status or the outcome
    // map. The engine's `apply_ack` already filters stale state acks; this
    // test pins that the queue-projection layer also stays put.
    let author = "88".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "77".repeat(32).as_str(),
        &author,
        1,
        "idempotence t128",
    );
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);

    // Settle both relays.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);
    assert_eq!(entry_for(&kernel, &signed.id).status, "ok");
    let outcomes_before = entry_for(&kernel, &signed.id).relay_outcomes.clone();

    // Late duplicate ack for r1 — engine has already evicted the in-flight
    // row, so `on_ack` is a no-op and `take_completed` returns nothing
    // → `set_publish_entry_terminal` is never called again
    // → the queue entry must be unchanged.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 1_000);
    let entry = entry_for(&kernel, &signed.id);
    assert_eq!(
        entry.status, "ok",
        "late ack must not perturb the terminal status"
    );
    assert_eq!(
        entry.relay_outcomes, outcomes_before,
        "late ack must not perturb the outcome map"
    );
}

#[test]
fn t128_terminal_status_survives_snapshot_round_trip_to_wire_json() {
    // End-to-end contract: drive a publish to terminal, take the snapshot
    // JSON, and assert the wire format carries the new `status` + the
    // `relay_outcomes` array. iOS Pulse `ComposeView` decodes off this exact
    // JSON (`KernelUpdate.publishQueue[…]`, computed from
    // `projections.publish_queue`), so this test is the contract line between
    // the kernel and the Swift side.
    let author = "aa".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "99".repeat(32).as_str(),
        &author,
        1,
        "wire-shape t128",
    );
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    let snapshot_json = kernel.make_update(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    // D0: the publish cluster is no longer a typed `KernelSnapshot` field —
    // `publish_queue` is a built-in entry in the host-extensible `projections`
    // map.
    let queue = parsed
        .get("projections")
        .and_then(|v| v.get("publish_queue"))
        .and_then(|v| v.as_array())
        .expect("projections.publish_queue must be present and an array");
    let entry = queue
        .iter()
        .find(|e| e.get("event_id").and_then(|v| v.as_str()) == Some(signed.id.as_str()))
        .expect("our publish must be in the wire snapshot");
    assert_eq!(
        entry.get("status").and_then(|v| v.as_str()),
        Some("ok"),
        "wire snapshot must surface the terminal status"
    );
    let outcomes = entry
        .get("relay_outcomes")
        .and_then(|v| v.as_array())
        .expect("relay_outcomes must serialize on a terminal entry");
    assert_eq!(outcomes.len(), 2);
    for outcome in outcomes {
        assert_eq!(outcome.get("status").and_then(|v| v.as_str()), Some("ok"));
        let url = outcome
            .get("relay_url")
            .and_then(|v| v.as_str())
            .expect("relay_url present");
        assert!(url == WRITE_R1 || url == WRITE_R2);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Direction review #24 — `projections.last_action_result`.
//
// `dispatch_action` fires `deliver_result` the instant the executor's
// channel-send returns `Ok` ("queued", not "published"). When a publish
// settles terminally (every relay landed Ok / FailedAfterRetries, the user
// cancelled, or no relays resolved) there was no terminal signal in the
// snapshot — a host spinner span permanently. `KernelSnapshot.projections`
// now carries a `"last_action_result"` key with the MOST RECENT terminal
// verdict so the host can clear its spinner without polling.
//
// `last_action_result` is a most-recent convenience signal; the authoritative
// per-correlation_id terminal state lives in `projections.publish_queue` via
// the T128 `set_publish_entry_terminal` path (covered above). The tests below
// pin both facts: the scalar shows the latest, AND `publish_queue` retains
// every terminal — concurrent settles in one tick are never lost.
// ───────────────────────────────────────────────────────────────────────────

/// Read `projections.last_action_result` from a fresh wire snapshot.
fn last_action_result(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("last_action_result"))
        .cloned()
        .expect("projections.last_action_result key must always be present")
}

#[test]
fn last_action_result_is_null_before_any_publish_settles() {
    // A kernel that has never settled a publish reports a `null` value — the
    // host treats null/absent as "no terminal result yet, keep the spinner".
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        last_action_result(&mut kernel).is_null(),
        "last_action_result must be null until the first action settles"
    );
}

#[test]
fn last_action_result_reports_published_on_all_ack_success() {
    // Every relay acks Ok → `last_action_result` is
    // `{status:"published", error:null}` keyed on the publish handle.
    let author = "a1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b1".repeat(32).as_str(), &author, 1, "publish ok");
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);

    // Not terminal after one ack — still null.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    assert!(
        last_action_result(&mut kernel).is_null(),
        "a partially-acked publish has no terminal result yet"
    );

    // Second ack settles it.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);
    let result = last_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "all-ack publish reports the wire status `published` (internal `ok`)"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "correlation_id is the publish handle (== event_id for publish actions)"
    );
    assert!(
        result.get("error").map(|v| v.is_null()).unwrap_or(false),
        "a published result carries a null error"
    );
}

#[test]
fn last_action_result_reports_failed_with_reason_on_all_relays_giving_up() {
    // Every relay burns through its retries → `last_action_result` is
    // `{status:"failed", error:"<joined per-relay reasons>"}`.
    let author = "a2".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b2".repeat(32).as_str(), &author, 1, "publish fail");
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);

    let drive_to_giveup = |kernel: &mut Kernel, relay: &str, base_ms: u64| {
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 1"),
            base_ms + 100,
        );
        let _ = kernel.tick_publish_engine(base_ms + 1_500);
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 2"),
            base_ms + 1_600,
        );
        let _ = kernel.tick_publish_engine(base_ms + 6_000);
        let _ = kernel.handle_publish_ok_at(
            relay,
            ok_payload(&signed.id, false, "io: down attempt 3"),
            base_ms + 6_100,
        );
    };
    drive_to_giveup(&mut kernel, WRITE_R1, 0);
    drive_to_giveup(&mut kernel, WRITE_R2, 100_000);

    let result = last_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "all-relays-give-up publish reports status `failed`"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str())
    );
    let error = result
        .get("error")
        .and_then(|v| v.as_str())
        .expect("a failed result must carry a non-null error string");
    assert!(
        error.contains("transient"),
        "the error must carry the per-relay give-up reason: {}",
        error
    );
}

#[test]
fn last_action_result_reports_failed_when_no_relays_resolve() {
    // No kind:10002 seeded → `Nip65OutboxResolver` resolves zero relays →
    // `emit_no_targets` runs and the publish never queues. This is a terminal
    // `failed` from the host's view; `last_action_result` must report it so
    // the spinner is cleared rather than spinning on an op that never ran.
    let author = "a3".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed("b3".repeat(32).as_str(), &author, 1, "no targets");
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);

    let result = last_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "a NoTargets publish is a terminal failure"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str())
    );
    assert!(
        result
            .get("error")
            .and_then(|v| v.as_str())
            .map(|e| e.contains("no relays resolved"))
            .unwrap_or(false),
        "the NoTargets error must explain that no relays were resolved"
    );
}

#[test]
fn last_action_result_reports_cancelled_on_user_cancel() {
    // User cancels an in-flight publish → `last_action_result` is
    // `{status:"cancelled", error:null}`. Cancellation never flows through
    // `recently_completed`, so the engine records `last_terminal` directly in
    // `cancel_publish` — this test pins that path.
    let author = "a4".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b4".repeat(32).as_str(), &author, 1, "cancel me");
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);

    kernel.cancel_publish(&signed.id);

    let result = last_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("cancelled"),
        "a user-cancelled publish reports status `cancelled`"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str())
    );
    assert!(
        result.get("error").map(|v| v.is_null()).unwrap_or(false),
        "a cancelled result carries a null error"
    );
}

#[test]
fn last_action_result_is_overwritten_by_the_most_recent_terminal() {
    // Two publishes settle in sequence — `last_action_result` is sticky-but-
    // latest: it reports the SECOND publish's verdict, not the first. This is
    // the deliberate "no queue, no history, no replay" tradeoff: the scalar is
    // a convenience signal; per-action authoritative state lives in
    // `publish_queue`.
    let author = "a5".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);

    let first = fake_signed("c1".repeat(32).as_str(), &author, 1, "first publish");
    let _ = kernel.run_publish_engine_at(&first, &[], crate::publish::PublishTarget::Auto, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&first.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&first.id, true, ""), 20);
    assert_eq!(
        last_action_result(&mut kernel)
            .get("correlation_id")
            .and_then(|v| v.as_str()),
        Some(first.id.as_str()),
        "after the first publish settles it is the latest terminal"
    );

    let second = fake_signed("c2".repeat(32).as_str(), &author, 1, "second publish");
    let _ = kernel.run_publish_engine_at(&second, &[], crate::publish::PublishTarget::Auto, 100);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&second.id, true, ""), 110);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&second.id, true, ""), 120);

    let result = last_action_result(&mut kernel);
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(second.id.as_str()),
        "last_action_result is overwritten with the most recent terminal"
    );
}

#[test]
fn concurrent_terminals_in_one_tick_keep_all_in_publish_queue() {
    // Coordinator's concern (review #25): if two publishes settle in the same
    // tick, the `last_action_result` scalar can only show one of them. This
    // test proves no terminal is LOST — the authoritative per-correlation_id
    // status lives in `projections.publish_queue` for BOTH, while
    // `last_action_result` shows the most recently settled. The host resolves
    // any other correlation_id by reading `publish_queue`.
    let author = "a6".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);

    let first = fake_signed("d1".repeat(32).as_str(), &author, 1, "concurrent first");
    let second = fake_signed("d2".repeat(32).as_str(), &author, 1, "concurrent second");
    let _ = kernel.run_publish_engine_at(&first, &[], crate::publish::PublishTarget::Auto, 0);
    let _ = kernel.run_publish_engine_at(&second, &[], crate::publish::PublishTarget::Auto, 0);

    // Settle BOTH publishes back-to-back (both terminal before any snapshot).
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&first.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&first.id, true, ""), 20);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&second.id, true, ""), 30);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&second.id, true, ""), 40);

    // Both terminals are retained in publish_queue — nothing is dropped.
    assert_eq!(
        entry_for(&kernel, &first.id).status,
        "ok",
        "the first concurrent publish's terminal status is retained in publish_queue"
    );
    assert_eq!(
        entry_for(&kernel, &second.id).status,
        "ok",
        "the second concurrent publish's terminal status is retained in publish_queue"
    );

    // last_action_result shows the most recently settled (second).
    assert_eq!(
        last_action_result(&mut kernel)
            .get("correlation_id")
            .and_then(|v| v.as_str()),
        Some(second.id.as_str()),
        "last_action_result shows the most recent terminal; publish_queue has the rest"
    );
}

#[test]
fn last_action_result_survives_apply_engine_completions_drain() {
    // Regression guard for the drain trap: `apply_engine_completions` drains
    // `recently_completed` via `take_completed()` inside every engine
    // entrypoint. `last_terminal` is a SEPARATE sticky field — it must NOT be
    // drained. After a settle, multiple snapshot reads in a row must all keep
    // reporting the terminal result.
    let author = "a7".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("e1".repeat(32).as_str(), &author, 1, "drain guard");
    let _ = kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    // Three consecutive snapshots — each calls make_update; the terminal
    // result must persist across all of them.
    for read in 0..3 {
        let result = last_action_result(&mut kernel);
        assert_eq!(
            result.get("status").and_then(|v| v.as_str()),
            Some("published"),
            "last_action_result must survive snapshot read #{} (no drain)",
            read
        );
    }
}
