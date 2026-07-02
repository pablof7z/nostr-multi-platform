//! Relay disconnection: clear wallet state and send a CLOSE frame.

use super::*;

use nmp_core::substrate::WalletKernelAccess;
use nmp_core::OutboundMessage;
use nmp_network::role::RelayRole;
use serde_json::json;

use super::runtime_utils::encode_frame;
use crate::payment_store::{PaymentRecord, PaymentState};
use crate::status::WalletStatus;

pub(super) fn wallet_disconnect_inner(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
) -> Vec<OutboundMessage> {
    let Some(conn) = wallet.connection.take() else {
        return Vec::new();
    };
    // Double-pay safety: a disconnect does NOT mean inflight payments failed.
    // The payment may settle on the wallet side after the subscription is gone;
    // the kind:23195 response simply won't reach us until we reconnect. So we
    // transition each inflight payment to the durable `Unknown` state (for
    // `lookup_invoice` reconciliation on reconnect) instead of recording a
    // failure that would let the user double-pay.
    for (request_id, entry) in conn.pending_payments.iter() {
        if let Some(store) = wallet.payment_store.as_ref() {
            let record = PaymentRecord {
                request_event_id: request_id.clone(),
                bolt11: entry.bolt11.clone(),
                correlation_id: entry.correlation_id.clone(),
                amount_msats: entry.amount_msats,
                state: PaymentState::Unknown,
                preimage: None,
            };
            if let Err(e) = store.upsert(&record) {
                tracing::warn!(
                    request_event_id = %request_id,
                    "nwc: failed to persist Unknown payment record on disconnect: {e}"
                );
            }
        }
        // Deliberately NOT calling record_action_failure — the action stays
        // in-flight until reconciliation settles it on reconnect.
    }
    kernel.unregister_persistent_sub(&conn.relay_url, &conn.sub_id);
    kernel.clear_relay_auth_signer(RelayRole::Wallet);
    // V-63: encode CLOSE frame; on failure log a toast but do not push an
    // empty-string frame (the subscription will expire on the relay side).
    let close_msg_opt = match encode_frame(&json!(["CLOSE", &conn.sub_id])) {
        Ok(msg) => Some(msg),
        Err(e) => {
            tracing::warn!("nwc: CLOSE frame encode failed: {e}");
            None
        }
    };
    // D6 poison-lock recovery — same as `sync_wallet_status`. Recover rather
    // than silently skipping the disconnect status write.
    {
        let mut slot = match wallet.status_slot.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("nwc: status_slot lock was poisoned on disconnect — recovering");
                poisoned.into_inner()
            }
        };
        let balance_sats = conn.balance_msats.map(|m| m / 1000);
        let wire = "disconnected";
        *slot = Some(WalletStatus {
            status: wire.to_string(),
            relay_url: conn.relay_url.clone(),
            wallet_pubkey_hex: conn.wallet_pubkey_hex.clone(),
            balance_msats: conn.balance_msats,
            balance_sats,
            is_ready: false,
            is_connected: false,
            connection_state: None,
        });
    }
    match close_msg_opt {
        Some(close_msg) => vec![OutboundMessage::new(
            RelayRole::Wallet,
            conn.relay_url,
            close_msg,
        )],
        None => Vec::new(),
    }
}
