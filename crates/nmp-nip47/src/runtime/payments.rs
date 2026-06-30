//! Payment lifecycle: TTL sweep of inflight `pay_invoice` entries and
//! `lookup_invoice` reconciliation of unresolved durable records.

use super::*;

use nmp_core::substrate::WalletKernelAccess;
use nmp_core::OutboundMessage;
use nmp_nwc::types::LookupInvoiceParams;
use nmp_nwc::NwcMethod;
use serde_json::json;

use crate::payment_store::{PaymentRecord, PaymentState};

/// Observational outcome of a TTL-expired `pay_invoice` entry swept by
/// [`WalletRuntime::sweep_expired_payments`].
///
/// The sweep has ALREADY transitioned the durable record to `Unknown` and
/// removed the in-memory entry. The caller MUST NOT call
/// `record_action_failure` on `correlation_id` — the payment may still settle
/// and is reconciled via `lookup_invoice`. These fields exist for logging and
/// future host-side diagnostics only.
pub struct ExpiredPaymentOutcome {
    /// The kind:23194 request event id of the expired payment.
    pub request_event_id: String,
    /// The dispatched action correlation id, if any. `None` for actor-internal
    /// auto-dispatched payments with no host spinner.
    pub correlation_id: Option<String>,
}

impl WalletRuntime {
    /// Sweep `pending_payments` entries older than `now_secs` by `ttl_secs`.
    ///
    /// ## Double-pay safety (the core fix)
    ///
    /// A TTL elapsing does NOT mean the payment failed — a lightning HTLC can
    /// stay in-flight for hours, and the kind:23195 response can arrive long
    /// after our 90 s sweep window. Recording a `Failed` terminal here would
    /// let a host show "payment failed", inviting the user to mint a fresh
    /// invoice and pay twice.
    ///
    /// So instead of returning failures, this sweep transitions each expired
    /// entry to the durable `Unknown` state (written to the payment store) and
    /// removes it from the in-memory map. The action stays in-flight from the
    /// host's perspective; reconciliation via `lookup_invoice` on the next
    /// reconnect resolves it to Succeeded or Failed. The returned outcomes are
    /// purely observational — the caller no longer calls `record_action_failure`.
    ///
    /// D8 — no sleep/loop: pure wall-clock compare of `now_secs` against the
    /// per-entry `inserted_at_secs` field.
    pub fn sweep_expired_payments(
        &mut self,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Vec<ExpiredPaymentOutcome> {
        let store = self.payment_store.as_ref();
        let conn = match self.connection.as_mut() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let expired_ids: Vec<String> = conn
            .pending_payments
            .iter()
            .filter(|(_, e)| now_secs.saturating_sub(e.inserted_at_secs) >= ttl_secs)
            .map(|(k, _)| k.clone())
            .collect();
        let mut outcomes: Vec<ExpiredPaymentOutcome> = Vec::new();
        for event_id in expired_ids {
            if let Some(entry) = conn.pending_payments.remove(&event_id) {
                tracing::warn!(
                    event_id = %event_id,
                    inserted_at_secs = entry.inserted_at_secs,
                    now_secs = now_secs,
                    ttl_secs = ttl_secs,
                    "nwc: pay_invoice TTL elapsed with no response — transitioning to \
                     Unknown for lookup_invoice reconciliation (NOT recording failure)"
                );
                // Transition the durable record to Unknown. The HTLC may still
                // settle; we must be able to reconcile, never declare failure.
                if let Some(store) = store {
                    let record = PaymentRecord {
                        request_event_id: event_id.clone(),
                        bolt11: entry.bolt11.clone(),
                        correlation_id: entry.correlation_id.clone(),
                        amount_msats: entry.amount_msats,
                        state: PaymentState::Unknown,
                        preimage: None,
                    };
                    if let Err(e) = store.upsert(&record) {
                        tracing::warn!(
                            event_id = %event_id,
                            "nwc: failed to persist Unknown payment record on TTL sweep: {e}"
                        );
                    }
                }
                outcomes.push(ExpiredPaymentOutcome {
                    request_event_id: event_id,
                    correlation_id: entry.correlation_id,
                });
            }
        }
        outcomes
    }
}

/// Issue a `lookup_invoice` for every unresolved (`PaySent`/`Unknown`) durable
/// record so payments whose outcome we missed (TTL, disconnect, restart) are
/// reconciled. Returns the outbound `lookup_invoice` frames; registers each in
/// `pending_lookups` so the reply maps back to the original payment.
pub(super) fn reconcile_unresolved_payments(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    relay: &str,
) -> Vec<OutboundMessage> {
    let Some(store) = wallet.payment_store.as_ref() else {
        return Vec::new();
    };
    let records = match store.load_unresolved() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("nwc: failed to load unresolved payments for reconciliation: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for record in records {
        let params = json!(LookupInvoiceParams {
            payment_hash: None,
            invoice: Some(record.bolt11.clone()),
        });
        // A reconciliation lookup is not itself a payment — no correlation id.
        if let Some((msg, lookup_request_id)) = super::request_builder::build_request(
            wallet,
            kernel,
            relay,
            NwcMethod::LookupInvoice,
            params,
            None,
        ) {
            if let Some(conn) = wallet.connection.as_mut() {
                conn.pending_lookups
                    .insert(lookup_request_id, record.request_event_id.clone());
            }
            out.push(msg);
        }
    }
    out
}
