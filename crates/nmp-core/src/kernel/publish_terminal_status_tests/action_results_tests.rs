//! Direction review #29 — `projections.action_results`.
//!
//! `dispatch_action` fires `deliver_result` the instant the executor's
//! channel-send returns `Ok` ("queued", not "published"). When a publish
//! settles terminally (every relay landed Ok / FailedAfterRetries, the user
//! cancelled, or no relays resolved) the host needs a terminal signal or its
//! spinner spins forever. `KernelSnapshot.projections` carries an
//! `"action_results"` array — every terminal verdict that settled since the
//! last emit — so the host can clear its spinner without polling.
//!
//! `action_results` is a per-tick DRAIN: two actions settling in one tick both
//! appear, neither is lost. The authoritative per-correlation_id terminal state
//! also lives in `projections.publish_queue` via the T128
//! `set_publish_entry_terminal` path (covered in `queue_status_tests`). The
//! tests below pin `action_results` surfacing every settled verdict,
//! including the `PublishRaw` minted-correlation_id round-trip and same-tick
//! concurrent drains.

use super::publish_terminal_status_support::*;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

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
    // `{status:"cancelled", error:null}`. The cancel terminal's
    // `PublishQueueTerminal::Cancelled` payload makes the single engine-terminal
    // fold both record the `cancelled` action result AND flip the queue row.
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
