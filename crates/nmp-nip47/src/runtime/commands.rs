//! Command handlers — the public surface the ProtocolCommands call into.
//!
//! Contains `wallet_connect`, `wallet_disconnect`, `wallet_pay_invoice`, and
//! `handle_nwc_text`.

use super::*;

use nmp_core::substrate::WalletKernelAccess;
use nmp_core::{AuthSignerFn, OutboundMessage};
use nmp_network::role::RelayRole;
use nmp_nwc::decode::{try_decode_relay_message_with_id, try_decode_response_for_request};
use nmp_nwc::parse::NwcUri;
use nmp_nwc::types::PayInvoiceParams;
use nmp_nwc::NwcMethod;
use nmp_signer_iface::UnsignedEvent;
use nostr::{Keys, SecretKey};
use serde_json::json;

use crate::crypto::sign_with;
use crate::reconcile::{correct_unresolved_record, settle_payment_failure, settle_payment_success};
use crate::status::NwcConnectionState;

use super::disconnect::wallet_disconnect_inner;
use super::heartbeat::sync_wallet_status;
use super::payments::reconcile_unresolved_payments;
use super::request_builder::{build_request, build_request_with_meta, PayMeta};
use super::runtime_utils::encode_frame;

// ── Command handlers (the public surface the ProtocolCommands call into) ─────

/// Parse a NWC URI and establish the connection state.
///
/// Wires the kernel-level NIP-47 infrastructure: a per-role NIP-42 signer for
/// [`RelayRole::Wallet`] using the NWC client secret, plus a persistent-sub
/// registration so EOSE doesn't auto-CLOSE the kind:23195 listener.
///
/// Returns outbound messages: a REQ subscription for kind:23195 and an
/// initial `get_info` + `get_balance` request to the NWC relay.
pub(crate) fn wallet_connect(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    uri: &str,
) -> Vec<OutboundMessage> {
    // Disconnect any existing connection first.
    if wallet.connection.is_some() {
        let _ = wallet_disconnect_inner(wallet, kernel);
    }

    let nwc_uri = match NwcUri::parse(uri) {
        Ok(u) => u,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::INVALID_URI,
                    format!("invalid NWC URI: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return Vec::new();
        }
    };

    let client_pubkey_hex = match nmp_nwc::crypto::client_pubkey_hex(&nwc_uri.client_secret_hex) {
        Ok(pk) => pk,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::INVALID_CLIENT_SECRET,
                    format!("invalid NWC client secret: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return Vec::new();
        }
    };

    let client_secret_key = match SecretKey::from_hex(&nwc_uri.client_secret_hex) {
        Ok(sk) => sk,
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::INVALID_CLIENT_SECRET,
                    format!("invalid NWC client secret: {e}"),
                )
                .with_detail(e.to_string()),
            );
            return Vec::new();
        }
    };

    let sub_id = format!("nwc-{}", &nwc_uri.wallet_pubkey_hex[..8]);
    let relay = nwc_uri.primary_relay_url().to_string();

    let conn = WalletConnection {
        wallet_pubkey_hex: nwc_uri.wallet_pubkey_hex.clone(),
        relay_url: relay.clone(),
        client_secret_hex: zeroize::Zeroizing::new(nwc_uri.client_secret_hex.as_str().to_string()),
        client_pubkey_hex: client_pubkey_hex.clone(),
        status: "connecting".to_string(),
        balance_msats: None,
        pending: std::collections::HashMap::new(),
        pending_payments: std::collections::HashMap::new(),
        pending_lookups: std::collections::HashMap::new(),
        sub_id: sub_id.clone(),
        orphan_responses: 0,
        last_probe_sent_secs: 0,
        probe_outstanding: false,
        consecutive_failures: 0,
        connection_state: None,
    };
    wallet.connection = Some(conn);

    // Bind the wallet-lane NIP-42 signer using the NWC client secret.
    let client_keys = Keys::new(client_secret_key);
    let signer: AuthSignerFn =
        std::sync::Arc::new(move |unsigned: &UnsignedEvent| sign_with(&client_keys, unsigned));
    kernel.set_relay_auth_signer(RelayRole::Wallet, client_pubkey_hex.clone(), signer);
    kernel.register_persistent_sub(relay.clone(), sub_id.clone());

    sync_wallet_status(wallet, kernel);

    let mut out = Vec::new();
    let req_filter = json!({
        "kinds": [23195u32],
        "authors": [&nwc_uri.wallet_pubkey_hex],
        "#p": [&client_pubkey_hex],
    });
    // V-63: encode before pushing. On failure set a toast and skip the frame
    // so no empty string is enqueued on the NWC relay.
    match encode_frame(&json!(["REQ", &sub_id, &req_filter])) {
        Ok(req_msg) => {
            out.push(OutboundMessage::new(
                RelayRole::Wallet,
                relay.clone(),
                req_msg,
            ));
        }
        Err(e) => {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::REQ_ENCODE_FAILED,
                    format!("NWC REQ encode failed: {e}"),
                )
                .with_detail(e.to_string()),
            );
        }
    }

    if let Some((msg, _id)) =
        build_request(wallet, kernel, &relay, NwcMethod::GetInfo, json!({}), None)
    {
        out.push(msg);
    }
    if let Some((msg, _id)) = build_request(
        wallet,
        kernel,
        &relay,
        NwcMethod::GetBalance,
        json!({}),
        None,
    ) {
        out.push(msg);
    }

    // Reconcile any payments left in PaySent/Unknown from a prior session or a
    // disconnect — issue a `lookup_invoice` per unresolved record so a payment
    // that settled while we were offline is corrected (never shown as failed).
    out.extend(reconcile_unresolved_payments(wallet, kernel, &relay));

    out
}

/// Clear wallet state and send a CLOSE to the NWC relay.
pub(crate) fn wallet_disconnect(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
) -> Vec<OutboundMessage> {
    wallet_disconnect_inner(wallet, kernel)
}

/// Sign and send a `pay_invoice` NWC request.
///
/// `correlation_id` carries the registry-minted action id when this call
/// originates from `nmp_app_dispatch_action` under `nmp.wallet.pay_invoice`;
/// `None` is reserved for actor-internal auto-dispatched payments where no
/// host spinner exists to close.
pub(crate) fn wallet_pay_invoice(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    bolt11: &str,
    amount_msats: Option<u64>,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    let conn = match &wallet.connection {
        Some(c) if c.status == "ready" => c,
        Some(_) => {
            let token = nmp_core::ui_token::UiToken::error(
                crate::ui_codes::WALLET_NOT_READY,
                "wallet not ready — still connecting",
            );
            kernel.set_last_error_token(&token);
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, token.fallback_prose().to_string());
            }
            return Vec::new();
        }
        None => {
            let token = nmp_core::ui_token::UiToken::error(
                crate::ui_codes::WALLET_NOT_CONNECTED,
                "no wallet connected",
            );
            kernel.set_last_error_token(&token);
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, token.fallback_prose().to_string());
            }
            return Vec::new();
        }
    };
    let relay = conn.relay_url.clone();
    let params = json!(PayInvoiceParams {
        invoice: bolt11.to_string(),
        amount: amount_msats,
    });
    let msg = build_request_with_meta(
        wallet,
        kernel,
        &relay,
        NwcMethod::PayInvoice,
        params,
        correlation_id.clone(),
        Some(PayMeta {
            bolt11: bolt11.to_string(),
            amount_msats,
        }),
    );
    match msg {
        Some((m, _id)) => vec![m],
        None => {
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, "NWC request build failed".to_string());
            }
            Vec::new()
        }
    }
}

// ── Relay message intercept ─────────────────────────────────────────────────

/// Called from the actor's relay-event handler when a text frame arrives
/// from the NWC relay. Decodes kind:23195 responses and updates state.
pub(crate) fn handle_nwc_text(
    wallet: &mut WalletRuntime,
    relay_text: &str,
    kernel: &dyn WalletKernelAccess,
) -> Vec<OutboundMessage> {
    // Split-borrow the two distinct fields so the payment-correlation arms can
    // touch the durable store while `conn` is mutably borrowed.
    let WalletRuntime {
        connection,
        payment_store,
        ..
    } = wallet;
    let conn = match connection.as_mut() {
        Some(c) => c,
        None => return Vec::new(),
    };
    let payment_store = payment_store.as_ref();

    let Some((_response_event_id, response)) = try_decode_relay_message_with_id(
        relay_text,
        &conn.wallet_pubkey_hex,
        conn.client_secret_hex.as_str(),
    ) else {
        return Vec::new();
    };

    // Drain `conn.pending` for ANY matched response (V-79 heartbeat probes,
    // get_balance, etc. — not just pay_invoice). The map is keyed by the
    // kind:23194 request id, which is the `e`-tag value the response carries,
    // NOT the response wrapper id. Without this drain `conn.pending` grew
    // unbounded (~2880 entries/day from 30 s heartbeats).
    let matched_request = try_decode_response_for_request(
        relay_text,
        &conn.wallet_pubkey_hex,
        conn.client_secret_hex.as_str(),
    );
    if let Some((req_id, _)) = &matched_request {
        conn.pending.remove(req_id);
    }

    if let Some(balance) = response.balance_msats() {
        conn.balance_msats = Some(balance);
        conn.status = "ready".to_string();
    }

    if response.result_type == "get_info" && response.error.is_none() {
        conn.status = "ready".to_string();
    }

    // V-79: any successful kind:23195 response means the relay is alive.
    // Reset the heartbeat failure counter and close the outstanding probe
    // flag regardless of which result_type arrived.
    if response.error.is_none() {
        conn.probe_outstanding = false;
        conn.consecutive_failures = 0;
        conn.connection_state = Some(NwcConnectionState::Connected);
    }

    if response.result_type == "pay_invoice" {
        if let Some((request_event_id, _response2)) = &matched_request {
            let entry_opt = conn.pending_payments.remove(request_event_id);
            match (&response.error, entry_opt) {
                (None, Some(entry)) => {
                    settle_payment_success(
                        payment_store,
                        request_event_id,
                        entry.correlation_id,
                        response.pay_preimage(),
                        kernel,
                    );
                }
                (Some(err), Some(entry)) => {
                    settle_payment_failure(
                        payment_store,
                        request_event_id,
                        entry.correlation_id,
                        &format!("{}: {}", err.code, err.message),
                        kernel,
                    );
                }
                // No live in-memory entry. This is NOT necessarily an orphan:
                // the entry may have been transitioned to `Unknown` by a TTL
                // sweep or a disconnect, or lost to a process restart. Correct
                // the durable record so a payment shown "in-flight" resolves to
                // its true outcome — preventing the double-pay vector.
                (err, None) => {
                    let corrected = correct_unresolved_record(
                        payment_store,
                        request_event_id,
                        err.is_none(),
                        response.pay_preimage(),
                        err.as_ref().map(|e| format!("{}: {}", e.code, e.message)),
                        kernel,
                    );
                    if !corrected {
                        conn.orphan_responses += 1;
                        tracing::warn!(
                            request_event_id = %request_event_id,
                            orphan_count = conn.orphan_responses,
                            "nwc: pay_invoice response arrived with no matching \
                             pending_payments entry and no durable record (orphan response)"
                        );
                    }
                }
            }
        }
    }

    // Reconciliation: a `lookup_invoice` reply correlates back to the ORIGINAL
    // payment via `pending_lookups` (its own `e` tag points at the lookup
    // request, not the payment request).
    if response.result_type == "lookup_invoice" {
        if let Some((lookup_request_id, _)) = &matched_request {
            if let Some(original_pay_id) = conn.pending_lookups.remove(lookup_request_id) {
                let lookup = response.lookup_invoice_result();
                let settled = lookup
                    .as_ref()
                    .and_then(|r| r.state.as_deref())
                    .map(|s| s == "settled")
                    .unwrap_or(false);
                let preimage = lookup.as_ref().and_then(|r| r.preimage.clone());
                if response.error.is_some() {
                    // The wallet has no record of this invoice → it was never
                    // paid. Safe to record a definitive failure now.
                    correct_unresolved_record(
                        payment_store,
                        &original_pay_id,
                        false,
                        None,
                        Some("lookup_invoice: not found".to_string()),
                        kernel,
                    );
                } else if settled {
                    correct_unresolved_record(
                        payment_store,
                        &original_pay_id,
                        true,
                        preimage,
                        None,
                        kernel,
                    );
                }
                // Not settled and not error → still pending on the wallet side;
                // leave the Unknown record in place to retry on a later reconnect.
            }
        }
    }

    if let Some(err) = &response.error {
        if err.code == "UNAUTHORIZED" || err.code == "RESTRICTED" {
            conn.status = "error".to_string();
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::WALLET_AUTH_ERROR,
                    format!("wallet error: {} — {}", err.code, err.message),
                )
                .with_subject(err.code.clone())
                .with_detail(err.message.clone()),
            );
        } else {
            kernel.set_last_error_token(
                &nmp_core::ui_token::UiToken::error(
                    crate::ui_codes::WALLET_ERROR,
                    format!("wallet: {} — {}", err.code, err.message),
                )
                .with_subject(err.code.clone())
                .with_detail(err.message.clone()),
            );
        }
    }

    sync_wallet_status(wallet, kernel);
    Vec::new()
}
