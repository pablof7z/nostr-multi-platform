//! `publish_queue_snapshot()` status/outcome-map transitions — T128's core
//! contract: all-ack success, all-relay give-up, partial success, late-ack
//! idempotence, wire-JSON round-trip (status + relay reasons), and
//! same-tick concurrent settles.

use super::publish_terminal_status_support::*;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn t128_all_relays_ack_flips_status_to_ok_with_full_outcome_map() {
    // Happy path: both NIP-65 write relays land OK acks → engine settles the
    // publish terminally → the engine-terminal fold flips the queue
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

    // Second ack — every relay has now settled → the engine records a terminal
    // carrying a `PublishQueueTerminal::Settled` payload → the single
    // engine-terminal fold derives the queue status → queue entry flips to `"ok"`.
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
    // row, so `on_ack` is a no-op and the engine-terminal fold drains nothing
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
