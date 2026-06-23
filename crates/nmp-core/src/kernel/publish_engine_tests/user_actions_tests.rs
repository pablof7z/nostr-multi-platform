#![cfg(test)]
//! User-driven publish action tests — manual retry and cancel operations.

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{fake_signed, ok_payload, seed_kind10002, WRITE_R1};

#[test]
fn user_retry_publish_now_dispatches_backoff_state() {
    let author = "ee".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("ef".repeat(32).as_str(), &author, 1, "manual retry");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let scheduled = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: temporary outage"),
        100,
    );
    assert!(
        scheduled.is_empty(),
        "transient failure should schedule backoff, not dispatch immediately"
    );

    let retry = kernel.retry_publish_now(&signed.id);
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].relay_url, WRITE_R1);
    assert!(
        retry[0].text.contains(&signed.id),
        "manual retry must re-dispatch the original signed event"
    );
}

#[test]
fn user_retry_publish_now_requeues_settled_failed_history_row() {
    let author = "e1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("e2".repeat(32).as_str(), &author, 1, "retry terminal");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let retry = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        100,
    );
    assert!(retry.is_empty());
    let failed = kernel.publish_queue_snapshot().last().unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.can_retry, "settled failure should expose retry");

    let retried = kernel.retry_publish_now(&signed.id);
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].relay_url, WRITE_R1);
    let rows: Vec<_> = kernel
        .publish_queue_snapshot()
        .iter()
        .filter(|entry| entry.event_id == signed.id)
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "retry should refine the existing history row"
    );
    assert_eq!(rows[0].status, "accepted_locally");
    assert!(!rows[0].can_retry);
}

#[test]
fn user_cancel_publish_clears_settled_history_row() {
    let author = "e3".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("e4".repeat(32).as_str(), &author, 1, "clear terminal");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let retry = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        100,
    );
    assert!(retry.is_empty());
    assert_eq!(
        kernel.publish_queue_snapshot().last().unwrap().status,
        "failed"
    );

    kernel.cancel_publish(&signed.id);

    assert!(
        kernel
            .publish_queue_snapshot()
            .iter()
            .all(|entry| entry.event_id != signed.id),
        "clear should remove the settled history row"
    );
}

#[test]
fn user_cancel_publish_removes_in_flight_and_store_intent() {
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let author = "ed".repeat(32);
    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("fa".repeat(32).as_str(), &author, 1, "cancel me");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    assert_eq!(kernel.publish_status_snapshot().in_flight.len(), 1);

    kernel.cancel_publish(&signed.id);

    assert!(
        kernel.publish_status_snapshot().in_flight.is_empty(),
        "cancel must remove the in-flight publish row"
    );
    assert!(
        publish_store.load_pending().unwrap().is_empty(),
        "cancel must delete the durable publish intent"
    );
    assert_eq!(
        kernel.publish_queue_snapshot().last().unwrap().status,
        "cancelled"
    );
}
