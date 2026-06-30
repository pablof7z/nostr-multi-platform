//! NWC request frame construction — sign, encode, and register in inflight maps.

use super::*;

use nmp_core::substrate::WalletKernelAccess;
use nmp_core::OutboundMessage;
use nmp_network::role::RelayRole;
use nmp_nwc::NwcMethod;
use serde_json::json;

use super::runtime_utils::encode_frame;
use crate::crypto::{build_event_json, sign_nwc_request};
use crate::payment_store::{PaymentRecord, PaymentState};

/// Metadata threaded into the `pay_invoice` tracking record.
pub(super) struct PayMeta {
    pub(super) bolt11: String,
    pub(super) amount_msats: Option<u64>,
}

/// Build a signed NWC request frame and register it in the inflight maps.
///
/// Returns `Some((outbound, request_event_id))` on success — the second tuple
/// element is the signed kind:23194 event id, which the `pay_invoice` caller
/// needs to correlate the durable [`PaymentRecord`] and (later) the
/// `lookup_invoice` reconciliation. Non-payment callers ignore it.
///
/// For `pay_invoice`, the durable record is written with state `PaySent`
/// BEFORE this returns (the record was already written by the caller; this fn
/// only registers the in-memory tracking). The bolt11/amount carried in
/// `pay_meta` are threaded into [`PendingPayment`] so a later TTL/disconnect
/// transition can write the `Unknown` record without re-deriving the invoice.
pub(super) fn build_request(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    relay_url: &str,
    method: NwcMethod,
    params: serde_json::Value,
    correlation_id: Option<String>,
) -> Option<(OutboundMessage, String)> {
    build_request_with_meta(
        wallet,
        kernel,
        relay_url,
        method,
        params,
        correlation_id,
        None,
    )
}

pub(super) fn build_request_with_meta(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    relay_url: &str,
    method: NwcMethod,
    params: serde_json::Value,
    correlation_id: Option<String>,
    pay_meta: Option<PayMeta>,
) -> Option<(OutboundMessage, String)> {
    let conn = wallet.connection.as_mut()?;

    let content = match nmp_nwc::build::request_content(
        conn.client_secret_hex.as_str(),
        &conn.wallet_pubkey_hex,
        &method,
        &params,
    ) {
        Ok(c) => c,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::ENCRYPT_FAILED,
                    format!("NWC encrypt: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return None;
        }
    };

    let created_at = kernel.now_secs();
    let signed = match sign_nwc_request(
        conn.client_secret_hex.as_str(),
        &conn.wallet_pubkey_hex,
        &content,
        created_at,
    ) {
        Ok(s) => s,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::SIGN_FAILED,
                    format!("NWC sign: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return None;
        }
    };

    let event_json = build_event_json(&signed);
    // V-63: encode the EVENT frame BEFORE inserting into pending maps.
    // If encoding fails we surface an error and return None without
    // registering the correlation_id as inflight — the pay_invoice path's
    // caller detects None and calls record_action_failure directly, so the
    // action is never left hanging.
    let text = match encode_frame(&json!(["EVENT", &event_json])) {
        Ok(t) => t,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::EVENT_ENCODE_FAILED,
                    format!("NWC EVENT encode failed: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return None;
        }
    };

    // Insert into tracking maps only after successful encoding (V-63).
    let request_event_id = signed.id.clone();
    let method_name = method.as_str().to_string();
    conn.pending.insert(request_event_id.clone(), method_name);
    if matches!(method, NwcMethod::PayInvoice) {
        let meta = pay_meta.unwrap_or(PayMeta {
            bolt11: String::new(),
            amount_msats: None,
        });

        // Double-pay safety (fail-closed): persist the PaySent record BEFORE
        // inserting into `pending_payments` and BEFORE returning the outbound
        // frame.  If the durable write fails we MUST NOT send the payment:
        // a payment with no durable record cannot be reconciled on restart and
        // creates a silent double-pay / balance-loss vector.  Return `None` so
        // the caller (`wallet_pay_invoice`) calls `record_action_failure`
        // instead of enqueuing the frame.
        //
        // When no `payment_store` is installed (unit tests / pre-startup) we
        // skip the write and proceed as before — the in-memory map is the only
        // tracking available in that mode.
        if let Some(store) = wallet.payment_store.as_ref() {
            let record = PaymentRecord {
                request_event_id: request_event_id.clone(),
                bolt11: meta.bolt11.clone(),
                correlation_id: correlation_id.clone(),
                amount_msats: meta.amount_msats,
                state: PaymentState::PaySent,
                preimage: None,
            };
            if let Err(e) = store.upsert(&record) {
                // Remove from `conn.pending` — we inserted it above but must
                // not leave a dangling diagnostic entry without a payment entry.
                conn.pending.remove(&request_event_id);
                tracing::error!(
                    request_event_id = %request_event_id,
                    "nwc: PaySent persist failed — aborting payment to prevent \
                     double-pay on restart: {e}"
                );
                kernel.set_last_error_token(
                    &nmp_core::ui_token::UiToken::error(
                        crate::ui_codes::PAYMENT_ABORTED_NO_DURABLE_RECORD,
                        format!("wallet: payment aborted — could not write durable record: {e}"),
                    )
                    .with_detail(e.to_string()),
                );
                return None;
            }
        }

        conn.pending_payments.insert(
            request_event_id.clone(),
            PendingPayment {
                correlation_id: correlation_id.clone(),
                inserted_at_secs: created_at,
                bolt11: meta.bolt11,
                amount_msats: meta.amount_msats,
            },
        );
    }

    Some((
        OutboundMessage::new(RelayRole::Wallet, relay_url.to_string(), text),
        request_event_id,
    ))
}
