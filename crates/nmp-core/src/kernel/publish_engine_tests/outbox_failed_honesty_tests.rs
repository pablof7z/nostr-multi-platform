#![cfg(test)]
//! #3022 regression coverage — a publish that permanently fails on every
//! targeted relay must surface as a distinct, actionable `"failed"` row in
//! `Kernel::publish_outbox_items()` / `outbox_summary_snapshot()`, not
//! silently vanish once `finalize_completed_rows` evicts it from
//! `PublishEngine::snapshot().in_flight`.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{fake_signed, ok_payload, seed_kind10002, WRITE_R1};

#[test]
fn permanently_failed_publish_surfaces_as_failed_outbox_row() {
    let author = "3022".repeat(16);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed(
        "b022".repeat(16).as_str(),
        &author,
        1,
        "this will never land",
    );

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);

    // The gap this closes: before the fix, `publish_outbox_items()` was a
    // pure projection of `in_flight`. A permanent, no-retry-budget-left
    // relay verdict (a non-retryable "blocked" reason on the ONLY targeted
    // relay) settles the row to `FailedAfterRetries` and
    // `finalize_completed_rows` evicts it from `in_flight` in the same tick
    // — so it must never simply disappear from the Outbox.
    let settle_result = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        100,
    );
    assert!(
        settle_result.is_empty(),
        "a permanent per-relay failure must not schedule a re-dispatch"
    );

    // Sanity: the row really is gone from the engine's in-flight set.
    assert!(
        kernel
            .publish_status_snapshot()
            .in_flight
            .iter()
            .all(|row| row.event_id != signed.id),
        "settled row must be evicted from in_flight"
    );

    let items = kernel.publish_outbox_items();
    let row = items.iter().find(|item| item.event_id == signed.id).expect(
        "a publish that permanently failed on every targeted relay must still \
             appear in publish_outbox_items() — the #3022 honesty gap",
    );
    assert_eq!(row.status, "failed");
    assert!(
        row.can_retry,
        "a failed row must carry a retry affordance (event id + can_retry)"
    );
    assert_eq!(
        row.handle, signed.id,
        "handle must resolve to the event id for retry_publish_now"
    );
    assert_eq!(row.target_relays, 1);
    assert_eq!(row.relays.len(), 1);
    assert_eq!(row.relays[0].relay_url, WRITE_R1);
    assert_eq!(row.relays[0].status, "failed");
    assert_eq!(row.content, "this will never land");

    let summary = kernel.outbox_summary_snapshot();
    assert_eq!(
        summary.failed, 1,
        "outbox_summary_snapshot must count the permanently-failed row"
    );
    assert_eq!(summary.total, 1);
    assert_eq!(summary.sending, 0);
    assert_eq!(summary.retrying, 0);
    assert_eq!(summary.queued, 0);

    // The row must still carry enough to act on it: retrying by the same
    // handle re-dispatches, proving the row is not a dead end.
    let retried = kernel.retry_publish_now(&signed.id);
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].relay_url, WRITE_R1);
}
