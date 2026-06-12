//! Durable-payment settlement + reconciliation helpers.
//!
//! These free functions are the terminal half of the double-pay-safe payment
//! lifecycle: they take a `pay_invoice` (or reconciling `lookup_invoice`)
//! outcome and (a) resolve the durable [`PaymentRecord`] in the store and
//! (b) close the dispatched action via the kernel. Extracted from `runtime.rs`
//! to keep that file under the 500-LOC ceiling (AGENTS.md), split by concrete
//! sub-protocol (payment reconciliation) under the same crate owner.

use nmp_core::Kernel;
use serde_json::json;

use crate::payment_store::FsPaymentStore;

/// Settle a payment to `Succeeded`: delete the durable record (terminal,
/// resolved) and close the dispatched action with the preimage (if any).
pub(crate) fn settle_payment_success(
    store: Option<&FsPaymentStore>,
    request_event_id: &str,
    correlation_id: Option<String>,
    preimage: Option<String>,
    kernel: &mut Kernel,
) {
    if let Some(store) = store {
        // Terminal + resolved → the record no longer needs reconciliation.
        if let Err(e) = store.delete(request_event_id) {
            tracing::warn!(
                request_event_id = %request_event_id,
                "nwc: failed to delete settled payment record: {e}"
            );
        }
    }
    if let Some(cid) = correlation_id {
        // The preimage IS the structured success result (it is the proof of
        // payment). Forward it as the action_results `result` body so a host
        // that dispatched the pay can surface the receipt.
        let result_json = preimage
            .as_ref()
            .map(|p| json!({ "preimage": p }).to_string());
        kernel.record_action_success(cid, result_json);
    }
}

/// Settle a payment to `Failed`: delete the durable record (terminal) and fail
/// the dispatched action.
pub(crate) fn settle_payment_failure(
    store: Option<&FsPaymentStore>,
    request_event_id: &str,
    correlation_id: Option<String>,
    reason: &str,
    kernel: &mut Kernel,
) {
    if let Some(store) = store {
        if let Err(e) = store.delete(request_event_id) {
            tracing::warn!(
                request_event_id = %request_event_id,
                "nwc: failed to delete failed payment record: {e}"
            );
        }
    }
    if let Some(cid) = correlation_id {
        kernel.record_action_failure(cid, reason.to_string());
    }
}

/// Correct a durable record whose in-memory entry is gone (TTL-swept,
/// disconnected, or restart-lost) when its true outcome finally arrives.
///
/// Returns `true` when a durable record was found and corrected (so the caller
/// does NOT count this as an orphan). Loads the record to recover its
/// `correlation_id`, then delegates to the success/failure settlement so the
/// dispatched action — still shown in-flight to the host — resolves correctly.
pub(crate) fn correct_unresolved_record(
    store: Option<&FsPaymentStore>,
    request_event_id: &str,
    success: bool,
    preimage: Option<String>,
    failure_reason: Option<String>,
    kernel: &mut Kernel,
) -> bool {
    let Some(store) = store else {
        return false;
    };
    let Ok(records) = store.load_unresolved() else {
        return false;
    };
    let Some(record) = records
        .into_iter()
        .find(|r| r.request_event_id == request_event_id)
    else {
        return false;
    };
    if success {
        settle_payment_success(
            Some(store),
            request_event_id,
            record.correlation_id,
            preimage,
            kernel,
        );
    } else {
        let reason = failure_reason.unwrap_or_else(|| "payment failed".to_string());
        settle_payment_failure(
            Some(store),
            request_event_id,
            record.correlation_id,
            &reason,
            kernel,
        );
    }
    true
}
