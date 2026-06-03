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
    // Use the author pubkey as the event id — guaranteed valid hex (64 hex
    // chars) and unique per author.  The old two-char prefix approach embedded
    // a literal 'k' which is not a valid hex character; V-70 strengthened
    // `is_structurally_valid()` to check hex chars, so those synthetic events
    // were rejected as Malformed and never entered the store.
    let id = author_pubkey.to_string();
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
fn entry_for<'a>(kernel: &'a Kernel, event_id: &str) -> &'a crate::kernel::PublishQueueEntry {
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
    let signed = fake_signed("11".repeat(32).as_str(), &author, 1, "all-ack t128");
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        None,
        1_000,
    );
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
    let signed = fake_signed("33".repeat(32).as_str(), &author, 1, "all-fail t128");
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
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
    let signed = fake_signed("55".repeat(32).as_str(), &author, 1, "partial t128");
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 2);

    // r1 settles OK on attempt 1.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);

    // Entry stays at `accepted_locally` — r2 is still in flight.
    assert_eq!(entry_for(&kernel, &signed.id).status, "accepted_locally");

    // r2 burns through three transient attempts.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, false, "io: down 1"), 100);
    let _ = kernel.tick_publish_engine(1_500);
    let _ =
        kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, false, "io: down 2"), 1_600);
    let _ = kernel.tick_publish_engine(6_000);
    let _ =
        kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, false, "io: down 3"), 6_100);

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
    let signed = fake_signed("77".repeat(32).as_str(), &author, 1, "idempotence t128");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

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
    let signed = fake_signed("99".repeat(32).as_str(), &author, 1, "wire-shape t128");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    let snapshot_json = kernel.make_update_json_for_test(true);
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

#[test]
fn t128_terminal_outcome_carries_relay_reason_to_wire_json() {
    // Regression guard for the per-relay rationale round-trip: the engine
    // captures `relay_reasons` from the resolver at publish time and stores
    // them on `InFlight`. `terminal_outcome_of` clones the map into
    // `TerminalOutcome`; `classify_terminal_outcome` then threads each reason
    // onto the corresponding `RelayAckOutcome`. Without that wiring the
    // settled `publish_queue` row would lose the "why was this relay
    // targeted?" string the in-flight `publish_outbox` row carries — chirp-tui
    // and iOS both render the history pane off this exact field, so a silent
    // drop would degrade UX with no test signal. Pin the contract here.
    let author = "bb".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("cc".repeat(32).as_str(), &author, 1, "reason roundtrip");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    let queue = parsed
        .get("projections")
        .and_then(|v| v.get("publish_queue"))
        .and_then(|v| v.as_array())
        .expect("projections.publish_queue must be an array");
    let entry = queue
        .iter()
        .find(|e| e.get("event_id").and_then(|v| v.as_str()) == Some(signed.id.as_str()))
        .expect("our publish must be in the wire snapshot");
    let outcomes = entry
        .get("relay_outcomes")
        .and_then(|v| v.as_array())
        .expect("relay_outcomes must serialize on a terminal entry");
    assert_eq!(outcomes.len(), 2);
    for outcome in outcomes {
        let reason = outcome.get("relay_reason").and_then(|v| v.as_str()).expect(
            "relay_reason must be present on every per-relay outcome — Nip65OutboxResolver \
                 emits a non-empty rationale for every write relay it returns",
        );
        // The NIP-65 resolver emits a stable English label for write relays;
        // assert on the substring rather than the exact wording so the
        // rationale copy can evolve without breaking this contract test.
        assert!(
            reason.to_ascii_lowercase().contains("write"),
            "relay_reason for a NIP-65 write relay must mention write; got {reason:?}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Direction review #29 — `projections.action_results`.
//
// `dispatch_action` fires `deliver_result` the instant the executor's
// channel-send returns `Ok` ("queued", not "published"). When a publish
// settles terminally (every relay landed Ok / FailedAfterRetries, the user
// cancelled, or no relays resolved) the host needs a terminal signal or its
// spinner spins forever. `KernelSnapshot.projections` carries an
// `"action_results"` array — every terminal verdict that settled since the
// last emit — so the host can clear its spinner without polling.
//
// `action_results` is a per-tick DRAIN: two actions settling in one tick both
// appear, neither is lost. The authoritative per-correlation_id terminal state
// also lives in `projections.publish_queue` via the T128
// `set_publish_entry_terminal` path (covered above). The tests below pin both:
// `action_results` surfaces every settled verdict AND `publish_queue` retains
// each terminal — concurrent settles in one tick are never lost.
// ───────────────────────────────────────────────────────────────────────────

/// Read `projections.action_results` from a fresh wire snapshot. The key is
/// conditionally inserted (only when a terminal settled this tick), so absence
/// is normal — it is reported as `Null` here.
fn action_results(kernel: &mut Kernel) -> serde_json::Value {
    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    parsed
        .get("projections")
        .and_then(|v| v.get("action_results"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// Drain `action_results` and assert exactly one terminal settled this tick,
/// returning it. Most terminal-status tests settle a single action.
fn single_action_result(kernel: &mut Kernel) -> serde_json::Value {
    let results = action_results(kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array when an action settled");
    assert_eq!(arr.len(), 1, "exactly one terminal settled this tick");
    arr[0].clone()
}

#[test]
fn action_results_reports_published_on_all_ack_success() {
    // Every relay acks Ok → `action_results` carries one
    // `{status:"published", error:null}` keyed on the publish handle.
    let author = "a1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b1".repeat(32).as_str(), &author, 1, "publish ok");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    // Not terminal after one ack — the key is absent.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    assert!(
        action_results(&mut kernel).is_null(),
        "a partially-acked publish has no terminal result yet"
    );

    // Second ack settles it.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);
    let result = single_action_result(&mut kernel);
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
fn action_results_reports_failed_with_reason_on_all_relays_giving_up() {
    // Every relay burns through its retries → `action_results` carries one
    // `{status:"failed", error:"<joined per-relay reasons>"}`.
    let author = "a2".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b2".repeat(32).as_str(), &author, 1, "publish fail");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

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

    let result = single_action_result(&mut kernel);
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
fn action_results_reports_failed_when_no_relays_resolve() {
    // No kind:10002 seeded → `Nip65OutboxResolver` resolves zero relays →
    // `emit_no_targets` runs and the publish never queues. This is a terminal
    // `failed` from the host's view; `action_results` must report it so
    // the spinner is cleared rather than spinning on an op that never ran.
    let author = "a3".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed("b3".repeat(32).as_str(), &author, 1, "no targets");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    let result = single_action_result(&mut kernel);
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
fn action_results_reports_cancelled_on_user_cancel() {
    // User cancels an in-flight publish → `action_results` carries one
    // `{status:"cancelled", error:null}`. Cancellation never flows through
    // `recently_completed`, so the engine records the terminal directly in
    // `cancel_publish` — this test pins that path.
    let author = "a4".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("b4".repeat(32).as_str(), &author, 1, "cancel me");
    let _ =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);

    kernel.cancel_publish(&signed.id);

    let result = single_action_result(&mut kernel);
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
fn concurrent_terminals_in_one_tick_keep_all_in_publish_queue() {
    // Coordinator's concern (review #25): if two publishes settle in the same
    // tick, no terminal is LOST — the authoritative per-correlation_id status
    // lives in `projections.publish_queue` for BOTH. The host resolves any
    // correlation_id by reading `publish_queue` (and `action_results` for the
    // per-tick spinner-clear signal, covered separately).
    let author = "a6".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);

    let first = fake_signed("d1".repeat(32).as_str(), &author, 1, "concurrent first");
    let second = fake_signed("d2".repeat(32).as_str(), &author, 1, "concurrent second");
    let _ = kernel.run_publish_engine_at(&first, &[], crate::publish::PublishTarget::Auto, None, 0);
    let _ =
        kernel.run_publish_engine_at(&second, &[], crate::publish::PublishTarget::Auto, None, 0);

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
}

#[test]
fn action_results_reports_dispatch_correlation_id_for_publish_raw() {
    // THE FIX (PublishRaw correlation_id round-trip): a `PublishRaw`
    // dispatch mints a random correlation_id because the event id is unknown
    // at dispatch time (the actor signs the event). When the publish settles,
    // the `action_results` entry's `correlation_id` MUST report that minted id
    // — not the signed event's id — so the host's spinner, keyed on the
    // dispatch return value, can be cleared.
    //
    // This drives `run_publish_engine_at` with an explicit
    // `correlation_id_override` (the path `commands::publish_unsigned_event` →
    // `publish_signed_with_correlation` takes once the actor has signed) and
    // asserts the projection reports the override verbatim.
    let author = "c9".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    // The signed kind:1 the actor produced — its id is the publish handle.
    let signed = fake_signed(
        "d9".repeat(32).as_str(),
        &author,
        1,
        "publishnote roundtrip",
    );
    // The registry-minted action correlation_id the host received from
    // `nmp_app_dispatch_action` — deliberately distinct from the event id.
    let minted_correlation_id = "9f".repeat(16);
    assert_ne!(
        minted_correlation_id, signed.id,
        "the test fixture must use a correlation_id distinct from the event id"
    );

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(minted_correlation_id.clone()),
        0,
    );
    // Settle both NIP-65 write relays.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 20);

    let result = single_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("published"),
        "the all-ack PublishRaw settles as `published`"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(minted_correlation_id.as_str()),
        "action_results must report the dispatch correlation_id, not the event id"
    );
    assert_ne!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(signed.id.as_str()),
        "the signed event id must NOT leak as the correlation_id for a PublishRaw"
    );
}

#[test]
fn action_results_reports_dispatch_correlation_id_on_publish_raw_failure() {
    // The override must also survive the failure path: a `PublishRaw` whose
    // relays all reject still has to report the minted correlation_id so the
    // host clears the spinner and shows the error against the right action.
    let author = "ca".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("da".repeat(32).as_str(), &author, 1, "publishnote fail");
    let minted_correlation_id = "7e".repeat(16);

    let _ = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        Some(minted_correlation_id.clone()),
        0,
    );
    // Both relays return a permanent NIP-20 rejection → terminal `failed`.
    let _ =
        kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, false, "blocked: spam"), 10);
    let _ =
        kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, false, "blocked: spam"), 20);

    let result = single_action_result(&mut kernel);
    assert_eq!(
        result.get("status").and_then(|v| v.as_str()),
        Some("failed"),
        "an all-reject PublishRaw settles as `failed`"
    );
    assert_eq!(
        result.get("correlation_id").and_then(|v| v.as_str()),
        Some(minted_correlation_id.as_str()),
        "the failure path must also report the dispatch correlation_id"
    );
}

#[test]
fn action_results_is_absent_before_any_publish_settles() {
    // Steady state: a kernel that has never settled a publish carries no
    // `action_results` key — the drain returns null and the projection is not
    // inserted. The host sees nothing to act on.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        action_results(&mut kernel).is_null(),
        "action_results must be absent until an action settles"
    );
}

#[test]
fn two_terminals_in_one_tick_both_appear_in_action_results() {
    // THE USER-BUG REGRESSION GUARD. Two publishes settle back-to-back, before
    // any snapshot is emitted. `action_results` must surface BOTH so the host
    // resolves every spinner, not just the most recent one.
    let author = "f1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);

    let first = fake_signed("e1".repeat(32).as_str(), &author, 1, "drain first");
    let second = fake_signed("e2".repeat(32).as_str(), &author, 1, "drain second");
    let _ = kernel.run_publish_engine_at(&first, &[], crate::publish::PublishTarget::Auto, None, 0);
    let _ =
        kernel.run_publish_engine_at(&second, &[], crate::publish::PublishTarget::Auto, None, 0);

    // Settle BOTH publishes before any snapshot emit — the same-tick condition.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&first.id, true, ""), 10);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&first.id, true, ""), 20);
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&second.id, true, ""), 30);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&second.id, true, ""), 40);

    // First (and only) snapshot read: action_results must carry BOTH verdicts.
    let results = action_results(&mut kernel);
    let arr = results
        .as_array()
        .expect("action_results must be a JSON array when actions settled");
    assert_eq!(
        arr.len(),
        2,
        "both terminals that settled in one tick must appear — neither is lost"
    );
    let mut ids: Vec<&str> = arr
        .iter()
        .filter_map(|item| item.get("correlation_id").and_then(|v| v.as_str()))
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![first.id.as_str(), second.id.as_str()],
        "both correlation_ids appear in action_results"
    );
    for item in arr {
        assert_eq!(
            item.get("status").and_then(|v| v.as_str()),
            Some("published"),
            "each all-OK settle reports the wire-level `published` status"
        );
        assert!(
            item.get("error").map(|v| v.is_null()).unwrap_or(false),
            "a successful publish carries a null error"
        );
    }

    // Drain semantics: the next snapshot tick (nothing new settled) carries no
    // `action_results` key — the spinner-resolution signal is consumed once.
    assert!(
        action_results(&mut kernel).is_null(),
        "action_results is drained per tick — a second read is absent"
    );
}
