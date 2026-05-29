//! NIP-47 Nostr Wallet Connect actor-side runtime.
//!
//! Moved from `nmp-core::actor::commands::wallet` in V-38. The runtime lives
//! on the actor thread; the actor reaches into it via the
//! [`WalletRuntimeHandle`] slot installed via
//! `nmp_core::NmpApp::set_wallet_runtime_handle`.
//!
//! D0: `nmp-core` no longer depends on `nmp-nwc`. D6: every error path
//! surfaces as a `last_error_toast` + `WalletStatus::status = "error"`,
//! never a panic.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::display::short_npub;
use nmp_core::substrate::UnsignedEvent;
use nmp_core::{AuthSignerFn, Kernel, OutboundMessage, RelayRole};
use nostr::nips::nip19::ToBech32;
use nostr::{Keys, PublicKey, SecretKey};
use serde_json::json;
use zeroize::Zeroizing;

use nmp_nwc::decode::{try_decode_relay_message_with_id, try_decode_response_for_request};
use nmp_nwc::parse::NwcUri;
use nmp_nwc::types::PayInvoiceParams;
use nmp_nwc::NwcMethod;

use crate::crypto::{build_event_json, sign_nwc_request, sign_with};
use crate::status::{format_sats_display, WalletStatus, WalletStatusSlot};

/// Actor-local NWC connection state. Cleared on `wallet_disconnect`.
struct WalletConnection {
    wallet_pubkey_hex: String,
    wallet_npub: String,
    relay_url: String,
    client_secret_hex: Zeroizing<String>,
    #[allow(dead_code)] // Retained for future per-event author filtering.
    client_pubkey_hex: String,
    status: String,
    balance_msats: Option<u64>,
    /// Inflight NWC requests: event_id → method name. Diagnostic-only.
    pending: HashMap<String, String>,
    /// Inflight `pay_invoice` requests keyed by the kind:23194 event id,
    /// value is the dispatched `correlation_id` (`Some` for FFI-dispatched
    /// pays — every wire path post-V3; `None` for actor-internal chains).
    /// Drained on the matching kind:23195 response to close the dispatched
    /// action promise.
    pending_payments: HashMap<String, Option<String>>,
    /// Sub-id used for the kind:23195 subscription on the NWC relay.
    sub_id: String,
}

/// Actor-thread-owned NWC runtime. Held behind a [`WalletRuntimeHandle`]
/// slot the actor reaches into per command and per relay message.
pub struct WalletRuntime {
    connection: Option<WalletConnection>,
    /// Shared output slot for the wallet projection. The actor (this runtime)
    /// is the sole writer (D4); the `"wallet"` snapshot projection reads it.
    status_slot: WalletStatusSlot,
}

impl std::fmt::Debug for WalletRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletRuntime")
            .field("connected", &self.connection.is_some())
            .finish_non_exhaustive()
    }
}

/// Shared, opaque, actor-owned wallet runtime handle. The three
/// [`crate::WalletConnectCommand`] / [`crate::WalletDisconnectCommand`] /
/// [`crate::WalletPayInvoiceCommand`] `ProtocolCommand` impls lock it inside
/// their `run` body; the relay-message intercept seam (the actor's
/// relay-event handler) does the same.
pub type WalletRuntimeHandle = Arc<Mutex<Option<WalletRuntime>>>;

/// Construct a fresh, empty [`WalletRuntimeHandle`]. The host installs it
/// via [`install_wallet_runtime`] at app startup; the actor's relay-text
/// intercept and the action-seam executor both pull it via
/// [`active_wallet_runtime`].
#[must_use]
pub fn new_wallet_runtime_handle() -> WalletRuntimeHandle {
    Arc::new(Mutex::new(None))
}

/// Process-wide slot holding the active [`WalletRuntimeHandle`]. There is
/// exactly one wallet runtime per process; the action-seam executor
/// ([`crate::WalletPayInvoiceModule::execute`]) and the FFI shims read it
/// here so they don't need an [`nmp_core::NmpApp`] reference. Hosts install
/// it once at app construction via [`install_wallet_runtime`].
///
/// Static rather than per-app because:
/// * `ActionModule::execute` is a `fn` (no `&self`, no `&NmpApp`);
/// * the FFI shims have an `app: *mut NmpApp` but no typed wallet field;
/// * the wallet runtime IS naturally process-scoped (the actor thread is
///   process-singleton too).
static ACTIVE_WALLET_RUNTIME: std::sync::OnceLock<WalletRuntimeHandle> =
    std::sync::OnceLock::new();

/// Install the process-wide wallet runtime handle. Must be called exactly
/// once per process; subsequent calls return `Err(_)` and the install is a
/// no-op (the first handle wins).
///
/// The host typically does this in its app-construction code, alongside
/// registering the `nmp.wallet.pay_invoice` action module and the
/// `"wallet"` snapshot projection.
pub fn install_wallet_runtime(handle: WalletRuntimeHandle) -> Result<(), &'static str> {
    ACTIVE_WALLET_RUNTIME
        .set(handle)
        .map_err(|_| "wallet runtime already installed")
}

/// Fetch a clone of the installed [`WalletRuntimeHandle`]. Returns `None`
/// when the host never called [`install_wallet_runtime`] — the action seam
/// then surfaces a `Failed` terminal stage rather than panicking (D6).
#[must_use]
pub fn active_wallet_runtime() -> Option<WalletRuntimeHandle> {
    ACTIVE_WALLET_RUNTIME.get().cloned()
}

impl WalletRuntime {
    /// Construct a wallet runtime bound to the shared status slot.
    #[must_use]
    pub fn new(status_slot: WalletStatusSlot) -> Self {
        Self {
            connection: None,
            status_slot,
        }
    }

    /// True if `relay_url` is the currently connected NWC relay. Used by
    /// the actor's relay-message intercept to decide whether to call
    /// [`handle_nwc_text`] for an inbound text frame.
    #[must_use]
    pub fn is_nwc_relay(&self, relay_url: &str) -> bool {
        self.connection
            .as_ref()
            .map(|c| c.relay_url == relay_url)
            .unwrap_or(false)
    }
}

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
    kernel: &mut Kernel,
    uri: &str,
) -> Vec<OutboundMessage> {
    // Disconnect any existing connection first.
    if wallet.connection.is_some() {
        let _ = wallet_disconnect_inner(wallet, kernel);
    }

    let nwc_uri = match NwcUri::parse(uri) {
        Ok(u) => u,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("invalid NWC URI: {e}")));
            return Vec::new();
        }
    };

    let client_pubkey_hex = match nmp_nwc::crypto::client_pubkey_hex(&nwc_uri.client_secret_hex) {
        Ok(pk) => pk,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("invalid NWC client secret: {e}")));
            return Vec::new();
        }
    };

    let client_secret_key = match SecretKey::from_hex(&nwc_uri.client_secret_hex) {
        Ok(sk) => sk,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("invalid NWC client secret: {e}")));
            return Vec::new();
        }
    };

    let wallet_npub = pubkey_to_npub(&nwc_uri.wallet_pubkey_hex).unwrap_or_else(|_| {
        nwc_uri.wallet_pubkey_hex[..8.min(nwc_uri.wallet_pubkey_hex.len())].to_string()
    });

    let sub_id = format!("nwc-{}", &nwc_uri.wallet_pubkey_hex[..8]);
    let relay = nwc_uri.primary_relay_url().to_string();

    let conn = WalletConnection {
        wallet_pubkey_hex: nwc_uri.wallet_pubkey_hex.clone(),
        wallet_npub: wallet_npub.clone(),
        relay_url: relay.clone(),
        client_secret_hex: Zeroizing::new(nwc_uri.client_secret_hex.as_str().to_string()),
        client_pubkey_hex: client_pubkey_hex.clone(),
        status: "connecting".to_string(),
        balance_msats: None,
        pending: HashMap::new(),
        pending_payments: HashMap::new(),
        sub_id: sub_id.clone(),
    };
    wallet.connection = Some(conn);

    // Bind the wallet-lane NIP-42 signer using the NWC client secret.
    let client_keys = Keys::new(client_secret_key);
    let signer: AuthSignerFn = Arc::new(move |unsigned: &UnsignedEvent| sign_with(&client_keys, unsigned));
    kernel.set_relay_auth_signer(RelayRole::Wallet, client_pubkey_hex.clone(), signer);
    kernel.register_persistent_sub(relay.clone(), sub_id.clone());

    sync_wallet_status(wallet, kernel);

    let mut out = Vec::new();
    let req_filter = json!({
        "kinds": [23195u32],
        "authors": [&nwc_uri.wallet_pubkey_hex],
        "#p": [&client_pubkey_hex],
    });
    let req_msg = serde_json::to_string(&json!(["REQ", &sub_id, &req_filter,])).unwrap_or_default();
    out.push(OutboundMessage::new(
        RelayRole::Wallet,
        relay.clone(),
        req_msg,
    ));

    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::GetInfo, json!({}), None) {
        out.push(msg);
    }
    if let Some(msg) =
        build_request(wallet, kernel, &relay, NwcMethod::GetBalance, json!({}), None)
    {
        out.push(msg);
    }

    out
}

/// Clear wallet state and send a CLOSE to the NWC relay.
pub(crate) fn wallet_disconnect(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    wallet_disconnect_inner(wallet, kernel)
}

fn wallet_disconnect_inner(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    let Some(conn) = wallet.connection.take() else {
        return Vec::new();
    };
    // Drain inflight dispatched `pay_invoice` correlation_ids BEFORE the
    // connection state is dropped — the kind:23195 response that would
    // have closed each dispatched action will never arrive once the
    // subscription is gone.
    for (_request_id, correlation_id_opt) in conn.pending_payments.iter() {
        if let Some(cid) = correlation_id_opt {
            kernel.record_action_failure(cid.clone(), "wallet disconnected".to_string());
        }
    }
    kernel.unregister_persistent_sub(&conn.relay_url, &conn.sub_id);
    kernel.clear_relay_auth_signer(RelayRole::Wallet);
    let close_msg = serde_json::to_string(&json!(["CLOSE", &conn.sub_id])).unwrap_or_default();
    if let Ok(mut slot) = wallet.status_slot.lock() {
        let balance_sats = conn.balance_msats.map(|m| m / 1000);
        *slot = Some(WalletStatus {
            status: "disconnected".to_string(),
            relay_url: conn.relay_url.clone(),
            wallet_npub: conn.wallet_npub.clone(),
            balance_msats: conn.balance_msats,
            balance_sats,
            balance_sats_display: balance_sats.map(format_sats_display),
            wallet_npub_short: short_npub(&conn.wallet_npub),
            is_ready: false,
            is_connected: false,
        });
    }
    vec![OutboundMessage::new(
        RelayRole::Wallet,
        conn.relay_url,
        close_msg,
    )]
}

/// Sign and send a `pay_invoice` NWC request.
///
/// `correlation_id` carries the registry-minted action id when this call
/// originates from `nmp_app_dispatch_action` under `nmp.wallet.pay_invoice`;
/// `None` is reserved for actor-internal auto-dispatched payments where no
/// host spinner exists to close.
pub(crate) fn wallet_pay_invoice(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
    bolt11: &str,
    amount_msats: Option<u64>,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    let conn = match &wallet.connection {
        Some(c) if c.status == "ready" => c,
        Some(_) => {
            let reason = "wallet not ready — still connecting".to_string();
            kernel.set_last_error_toast(Some(reason.clone()));
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, reason);
            }
            return Vec::new();
        }
        None => {
            let reason = "no wallet connected".to_string();
            kernel.set_last_error_toast(Some(reason.clone()));
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, reason);
            }
            return Vec::new();
        }
    };
    let relay = conn.relay_url.clone();
    let params = json!(PayInvoiceParams {
        invoice: bolt11.to_string(),
        amount: amount_msats,
    });
    let msg = build_request(
        wallet,
        kernel,
        &relay,
        NwcMethod::PayInvoice,
        params,
        correlation_id.clone(),
    );
    match msg {
        Some(m) => vec![m],
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
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    let conn = match wallet.connection.as_mut() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let Some((_response_event_id, response)) = try_decode_relay_message_with_id(
        relay_text,
        &conn.wallet_pubkey_hex,
        conn.client_secret_hex.as_str(),
    ) else {
        return Vec::new();
    };

    if let Some(balance) = response.balance_msats() {
        conn.balance_msats = Some(balance);
        conn.status = "ready".to_string();
    }

    if response.result_type == "get_info" && response.error.is_none() {
        conn.status = "ready".to_string();
    }

    if response.result_type == "pay_invoice" {
        let matched = try_decode_response_for_request(
            relay_text,
            &conn.wallet_pubkey_hex,
            conn.client_secret_hex.as_str(),
        );
        if let Some((request_event_id, _response2)) = matched {
            let correlation_id_opt = conn.pending_payments.remove(&request_event_id);
            match (&response.error, correlation_id_opt.and_then(|x| x)) {
                (None, Some(correlation_id)) => {
                    kernel.record_action_success(correlation_id);
                }
                (Some(err), Some(correlation_id)) => {
                    let reason = format!("{}: {}", err.code, err.message);
                    kernel.record_action_failure(correlation_id, reason);
                }
                (_, None) => {}
            }
        }
    }

    if let Some(err) = &response.error {
        if err.code == "UNAUTHORIZED" || err.code == "RESTRICTED" {
            conn.status = "error".to_string();
            kernel.set_last_error_toast(Some(format!(
                "wallet error: {} — {}",
                err.code, err.message
            )));
        } else {
            kernel.set_last_error_toast(Some(format!("wallet: {} — {}", err.code, err.message)));
        }
    }

    sync_wallet_status(wallet, kernel);
    Vec::new()
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn build_request(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
    relay_url: &str,
    method: NwcMethod,
    params: serde_json::Value,
    correlation_id: Option<String>,
) -> Option<OutboundMessage> {
    let conn = wallet.connection.as_mut()?;

    let content = match nmp_nwc::build::request_content(
        conn.client_secret_hex.as_str(),
        &conn.wallet_pubkey_hex,
        &method,
        &params,
    ) {
        Ok(c) => c,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("NWC encrypt: {e}")));
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
            kernel.set_last_error_toast(Some(format!("NWC sign: {e}")));
            return None;
        }
    };

    let method_name = method.as_str().to_string();
    conn.pending.insert(signed.id.clone(), method_name);
    if matches!(method, NwcMethod::PayInvoice) {
        conn.pending_payments
            .insert(signed.id.clone(), correlation_id);
    }

    let event_json = build_event_json(&signed);
    let text = serde_json::to_string(&json!(["EVENT", &event_json])).unwrap_or_default();

    Some(OutboundMessage::new(
        RelayRole::Wallet,
        relay_url.to_string(),
        text,
    ))
}

fn sync_wallet_status(wallet: &WalletRuntime, kernel: &mut Kernel) {
    let status = wallet.connection.as_ref().map(|c| {
        let balance_sats = c.balance_msats.map(|m| m / 1000);
        WalletStatus {
            status: c.status.clone(),
            relay_url: c.relay_url.clone(),
            wallet_npub: c.wallet_npub.clone(),
            balance_msats: c.balance_msats,
            balance_sats,
            balance_sats_display: balance_sats.map(format_sats_display),
            wallet_npub_short: short_npub(&c.wallet_npub),
            is_ready: c.status == "ready",
            is_connected: c.status == "connecting" || c.status == "ready",
        }
    });
    if let Ok(mut slot) = wallet.status_slot.lock() {
        *slot = status;
    }
    kernel.mark_changed_since_emit();
}

fn pubkey_to_npub(hex: &str) -> Result<String, String> {
    PublicKey::from_hex(hex)
        .map_err(|e| format!("{e}"))?
        .to_bech32()
        .map_err(|e| format!("{e}"))
}

