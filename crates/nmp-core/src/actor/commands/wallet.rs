//! NIP-47 Nostr Wallet Connect actor-side runtime.
//!
//! `WalletRuntime` is the actor-local wallet state. It manages the NWC connection,
//! builds kind:23194 request events, and decodes incoming kind:23195 responses.
//!
//! D0: `nmp-core` may depend on `nmp-nwc` (the protocol crate). The inverse is
//! not true. The kernel is kept protocol-neutral. NIP-47 NWC is an app noun, so
//! wallet state is NOT baked into `KernelSnapshot`: the actor writes it to a
//! shared [`WalletStatusSlot`] and a host-registered snapshot projection
//! (`projections["wallet"]`) reads it on every tick (D0 — the kernel emits,
//! never names a host noun).
//!
//! D6: all error paths surface as a `last_error_toast` + `WalletStatus::status = "error"`,
//! never as panics or FFI exceptions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nostr::nips::nip19::ToBech32;
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use serde::Serialize;
use serde_json::json;
use zeroize::Zeroizing;

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole};
use crate::substrate::{SignedEvent, UnsignedEvent};
use nmp_nwc::decode::{try_decode_relay_message_with_id, try_decode_response_for_request};
use nmp_nwc::parse::NwcUri;
use nmp_nwc::types::PayInvoiceParams;
use nmp_nwc::NwcMethod;

use super::identity::sign_with;

/// Actor-local NWC connection state. Cleared on `WalletDisconnect`.
struct WalletConnection {
    wallet_pubkey_hex: String,
    wallet_npub: String,
    relay_url: String,
    client_secret_hex: Zeroizing<String>,
    #[allow(dead_code)] // Retained for future per-event author filtering.
    client_pubkey_hex: String,
    status: String,
    balance_msats: Option<u64>,
    /// Inflight NWC requests: event_id → method name. Diagnostic-only mapping
    /// — informs no behaviour today but kept so a future telemetry layer can
    /// surface "what did the last `get_info` look like" without a wire-frame
    /// replay.
    pending: HashMap<String, String>,
    /// Inflight `pay_invoice` requests keyed by the kind:23194 event id, value
    /// is the dispatch correlation_id (`Some(id)` for actions originating from
    /// `nmp_app_dispatch_action` — every FFI path today, including the thin
    /// `nmp_app_wallet_pay_invoice` C-ABI wrapper post-V3; `None` for
    /// actor-internal auto-dispatched payments such as the
    /// `commands/zap.rs` LNURL → pay_invoice chain). On the matching
    /// kind:23195 response the entry is drained; `Some(id)` routes to
    /// [`Kernel::record_action_success`] / [`Kernel::record_action_failure`]
    /// so the host spinner clears, `None` is a no-op on the action_results
    /// side (the toast + balance refresh still fire).
    ///
    /// Separated from `pending` so the existing diagnostic map's shape
    /// (`HashMap<String, String>`) is unchanged — `pending_payments` carries
    /// the additional `Option<String>` payload only `pay_invoice` needs.
    /// Per-connection (not per-`WalletRuntime`) so a `WalletDisconnect`
    /// transparently drops every inflight payment id with the connection that
    /// originated them.
    pending_payments: HashMap<String, Option<String>>,
    /// Sub-id we used for the kind:23195 subscription on the NWC relay.
    sub_id: String,
}

/// NIP-47 wallet connection status — the app noun projected onto the snapshot
/// under `projections["wallet"]`.
///
/// D0: NIP-47 NWC is an app noun, not a kernel primitive. This type lives in
/// the wallet runtime (an app-noun module gated behind the `wallet` feature),
/// NOT in `KernelSnapshot`. The actor writes it to a [`WalletStatusSlot`]; a
/// host-registered snapshot projection serializes it into the snapshot's
/// `projections` map every tick.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct WalletStatus {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    pub(crate) status: String,
    /// The NWC relay URL (from the connection URI).
    pub(crate) relay_url: String,
    /// The wallet service pubkey in bech32 npub form.
    pub(crate) wallet_npub: String,
    /// Balance in millisatoshis, if the wallet has responded to `get_balance`.
    pub(crate) balance_msats: Option<u64>,
}

/// Shared wallet-status slot — the output side of the wallet projection.
///
/// One `Arc` clone lives on the actor's [`WalletRuntime`] (the sole writer,
/// D4); another is captured by the `"wallet"` snapshot-projection closure
/// registered on `NmpApp`. The projection reads this slot on every snapshot
/// tick and serializes its contents into `KernelSnapshot::projections`.
///
/// `None` (the default) means no wallet has been connected this session — the
/// projection then contributes JSON `null` under the `"wallet"` key,
/// preserving the "key present, value null when disconnected" semantic the
/// social shells already decode.
pub(crate) type WalletStatusSlot = Arc<Mutex<Option<WalletStatus>>>;

/// Construct a fresh, empty [`WalletStatusSlot`].
pub(crate) fn new_wallet_status_slot() -> WalletStatusSlot {
    Arc::new(Mutex::new(None))
}

pub(crate) struct WalletRuntime {
    connection: Option<WalletConnection>,
    /// Shared output slot for the wallet projection. The actor (this runtime)
    /// is the sole writer (D4); the `"wallet"` snapshot projection reads it.
    status_slot: WalletStatusSlot,
}

impl WalletRuntime {
    /// Construct a wallet runtime bound to the shared status slot.
    ///
    /// `status_slot` is the `Arc<Mutex<…>>` the actor writes wallet state into
    /// and the `"wallet"` snapshot projection reads from. The two `Arc` clones
    /// share one inner `Mutex`, so an actor write is visible to the projection
    /// closure on the next tick without crossing the FFI boundary.
    pub(crate) fn new(status_slot: WalletStatusSlot) -> Self {
        Self {
            connection: None,
            status_slot,
        }
    }

    /// True if `relay_url` is the currently connected NWC relay.
    pub(crate) fn is_nwc_relay(&self, relay_url: &str) -> bool {
        self.connection
            .as_ref()
            .map(|c| c.relay_url == relay_url)
            .unwrap_or(false)
    }
}

// ── Command handlers ──────────────────────────────────────────────────────────

/// Parse a NWC URI and establish the connection state.
///
/// Wires the kernel-level NIP-47 infrastructure:
/// - registers a per-role NIP-42 signer for `RelayRole::Wallet` using the NWC
///   client secret (the kernel answers AUTH challenges from the wallet relay
///   with this key, NOT the user's identity);
/// - registers the kind:23195 sub-id as persistent so EOSE doesn't auto-CLOSE
///   the listener.
///
/// Returns outbound messages: a REQ subscription for kind:23195 and an
/// initial `get_info` + `get_balance` requests to the NWC relay.
pub(crate) fn wallet_connect(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
    uri: &str,
) -> Vec<OutboundMessage> {
    // Disconnect any existing connection first (also tears down kernel-side
    // wallet-lane signer + persistent-sub registrations).
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

    // Bind the wallet-lane NIP-42 signer. The kernel's existing AUTH driver
    // will invoke this when the wallet relay (e.g. relay.damus.io) issues a
    // challenge — using the NWC client secret, never the user identity.
    let client_keys = Keys::new(client_secret_key);
    kernel.set_relay_auth_signer(
        RelayRole::Wallet,
        client_pubkey_hex.clone(),
        Arc::new(move |unsigned: &UnsignedEvent| sign_with(&client_keys, unsigned)),
    );
    // Pin the kind:23195 listener so EOSE doesn't auto-CLOSE it.
    kernel.register_persistent_sub(relay.clone(), sub_id.clone());

    sync_wallet_status(wallet, kernel);

    let mut out = Vec::new();

    // Subscribe for kind:23195 responses from the wallet.
    let req_filter = json!({
        "kinds": [23195u32],
        "authors": [&nwc_uri.wallet_pubkey_hex],
        "#p": [&client_pubkey_hex],
    });
    let req_msg = serde_json::to_string(&json!(["REQ", &sub_id, &req_filter,])).unwrap_or_default();
    out.push(OutboundMessage {
        role: RelayRole::Wallet,
        relay_url: relay.clone(),
        text: req_msg,
    });

    // Send get_info and get_balance immediately. Neither is dispatched via
    // `nmp_app_dispatch_action` (session bootstrap, not a host action), so
    // `correlation_id` is `None` — `build_request` ignores it for non-
    // PayInvoice methods anyway.
    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::GetInfo, json!({}), None) {
        out.push(msg);
    }
    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::GetBalance, json!({}), None)
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
    // Drain any inflight `pay_invoice` correlation_ids BEFORE the connection
    // state is dropped — the kind:23195 response that would have closed each
    // dispatched action will never arrive once the subscription is gone. Same
    // broken-promise pattern as the `wallet not ready` early-exit in
    // `wallet_pay_invoice`: a dispatched payment that does not reach the wire
    // (or whose acknowledgement path is torn down) must terminate the host
    // spinner here rather than silently leaking. `None` slots (C-ABI direct
    // callers) are skipped — no host promise to honour.
    for (_request_id, correlation_id_opt) in conn.pending_payments.iter() {
        if let Some(cid) = correlation_id_opt {
            kernel.record_action_failure(cid.clone(), "wallet disconnected".to_string());
        }
    }
    // Tear down kernel-side wallet-lane registrations.
    kernel.unregister_persistent_sub(&conn.relay_url, &conn.sub_id);
    kernel.clear_relay_auth_signer(RelayRole::Wallet);
    let close_msg = serde_json::to_string(&json!(["CLOSE", &conn.sub_id])).unwrap_or_default();
    // D4: actor is sole writer of the wallet status slot. Project a final
    // `disconnected` status (the snapshot's `"wallet"` projection reads this).
    if let Ok(mut slot) = wallet.status_slot.lock() {
        *slot = Some(WalletStatus {
            status: "disconnected".to_string(),
            relay_url: conn.relay_url.clone(),
            wallet_npub: conn.wallet_npub.clone(),
            balance_msats: conn.balance_msats,
        });
    }
    vec![OutboundMessage {
        role: RelayRole::Wallet,
        relay_url: conn.relay_url,
        text: close_msg,
    }]
}

/// Sign and send a `pay_invoice` NWC request.
///
/// `correlation_id` is the registry-minted action id when this call originates
/// from `nmp_app_dispatch_action` under namespace `nmp.wallet.pay_invoice`
/// (every FFI path today, post-V3 — the C-ABI symbol
/// `nmp_app_wallet_pay_invoice` is a thin wrapper that routes through the
/// action seam). The runtime stores `kind23194_event_id → correlation_id` in
/// [`WalletConnection::pending_payments`] so the matching kind:23195 response
/// in `handle_nwc_text` can drain it and route the outcome to
/// [`Kernel::record_action_success`] / [`Kernel::record_action_failure`].
/// `None` is reserved for actor-internal auto-dispatched payments (e.g. the
/// `commands/zap.rs` LNURL → pay_invoice chain) where no host spinner exists
/// to close — the toast + balance refresh still fire on the response, but no
/// `action_results` entry is produced.
///
/// Early-exit failures (wallet not connected / not ready) on a dispatched
/// action surface the terminal `Failed` into `action_results` immediately —
/// without this the host spinner would hang on a request that was never put
/// on the wire. `None`-carrier early exits remain toast-only (the
/// actor-internal caller has no spinner to clear).
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
            // `build_request` already surfaced a toast for the sign / encrypt
            // failure path. Close the dispatched-action promise too — the
            // payment request never reached the wire, so the kind:23195
            // response that would have drained `pending_payments` will never
            // arrive. Toast-only for non-dispatch callers.
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, "NWC request build failed".to_string());
            }
            Vec::new()
        }
    }
}

// ── Relay message intercept ───────────────────────────────────────────────────

/// Called from `handle_relay_event` when a message arrives from the NWC relay.
///
/// Parses kind:23195 EVENT frames, decrypts the content, and updates wallet
/// state. For `pay_invoice` responses, drains the matching entry from
/// [`WalletConnection::pending_payments`] and routes the outcome (preimage on
/// success, NWC `error` object on failure) into [`Kernel::record_action_success`]
/// / [`Kernel::record_action_failure`] so a dispatched-action spinner clears.
/// Returns any outbound messages (e.g. follow-up requests).
pub(crate) fn handle_nwc_text(
    wallet: &mut WalletRuntime,
    relay_text: &str,
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    let conn = match wallet.connection.as_mut() {
        Some(c) => c,
        None => return Vec::new(),
    };

    // Decode with the lenient matcher — bootstrap responses (`get_info`,
    // `get_balance`) historically did not require an `e` tag to be accepted
    // and some shipped wallets (Alby, Mutiny, Zeus, Coinos) may not include
    // one on the get_info bootstrap. The strict NIP-47 §3.2 matcher is
    // applied below only for `pay_invoice` responses, where the request-id
    // correlation is mandatory.
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

    // NIP-47 `pay_invoice` round-trip: this is the response that closes the
    // payment promise. Drain `pending_payments` BEFORE the generic error
    // branch below so the per-payment `correlation_id` carries the typed
    // failure into `action_results` even when the response is an error —
    // otherwise the host spinner would hang and the user could retry-pay the
    // same invoice (the root double-pay bug in the bug report).
    //
    // For pay_invoice we use the strict decoder (`try_decode_response_for_request`)
    // because NIP-47 §3.2 mandates an `e` tag for response correlation, and
    // here we need the **request** id (not the response wrapper's own id) to
    // look up `pending_payments`. A response missing its `e` tag cannot be
    // matched to any inflight payment — that's a wallet-service bug, but the
    // toast + balance refresh still fire below (D6 — silent on unmatchable,
    // never panic).
    //
    // The diagnostic `pending` map (event_id → method_name) is left untouched
    // here — it predates this fix and is not yet driven off, so changing its
    // lifecycle stays out of scope.
    if response.result_type == "pay_invoice" {
        let matched = try_decode_response_for_request(
            relay_text,
            &conn.wallet_pubkey_hex,
            conn.client_secret_hex.as_str(),
        );
        if let Some((request_event_id, _response2)) = matched {
            // Drain the per-payment correlation_id slot. `None` value is a
            // no-op on the action_results side — actor-internal
            // auto-dispatched payments (e.g. the LNURL → pay_invoice chain
            // in `commands/zap.rs`) have no host spinner to close — but the
            // toast + balance refresh still fire below. A missing entry
            // (duplicate response we already drained, or `e` tag pointing
            // to an unknown request id) is tolerated. Post-V3 the C-ABI
            // `nmp_app_wallet_pay_invoice` symbol routes through
            // `dispatch_action` and therefore always carries `Some(id)`.
            let correlation_id_opt = conn.pending_payments.remove(&request_event_id);
            match (&response.error, correlation_id_opt.and_then(|x| x)) {
                (None, Some(correlation_id)) => {
                    // Successful payment with a dispatched-action id waiting.
                    // Record the terminal so `action_results` carries
                    // `{status:"published"}` on the next emit; the host
                    // spinner clears.
                    kernel.record_action_success(correlation_id);
                }
                (Some(err), Some(correlation_id)) => {
                    // Failed payment with a dispatched-action id waiting.
                    // Record the typed failure verbatim so the host can
                    // display the NWC error message (`PAYMENT_FAILED`,
                    // `INSUFFICIENT_BALANCE`, etc.) instead of a generic
                    // timeout.
                    let reason = format!("{}: {}", err.code, err.message);
                    kernel.record_action_failure(correlation_id, reason);
                }
                // No dispatched correlation_id to close — either a C-ABI
                // direct call or an unknown event_id. The toast (set in the
                // generic branch below for error responses) is sufficient.
                (_, None) => {}
            }
        }
        // A pay_invoice response without an `e` tag is malformed per
        // NIP-47 §3.2 — we have no way to correlate to any inflight payment,
        // so action_results is simply not touched. The toast still fires
        // below for the error case; success silently surfaces only via the
        // balance refresh (next `get_balance` round-trip will reconcile).
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

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Build a signed kind:23194 request and return it as an `OutboundMessage`.
///
/// `correlation_id` is only meaningful for `NwcMethod::PayInvoice` — when
/// `Some`, the signed event id is recorded in
/// [`WalletConnection::pending_payments`] so `handle_nwc_text` can close the
/// dispatched-action promise on the matching kind:23195 response. Other
/// methods (`get_info`, `get_balance`, `make_invoice`) never round-trip a
/// correlation_id today — their callers are session bootstrap, not dispatched
/// actions — so the value is ignored for those branches and the map stays
/// scoped to payments.
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
    // Record the per-payment correlation id so the kind:23195 response (kept
    // by `handle_nwc_text`) can close the dispatched-action promise. Scope is
    // intentionally `PayInvoice` only — every other NWC method's caller is
    // session bootstrap with no host spinner to clear. The map carries an
    // `Option` value so a C-ABI direct call still owns its slot in the map
    // (and the response handler can still drain it for `sync_wallet_status`),
    // it just contributes nothing to `action_results`.
    if matches!(method, NwcMethod::PayInvoice) {
        conn.pending_payments
            .insert(signed.id.clone(), correlation_id);
    }

    let event_json = build_event_json(&signed);
    let text = serde_json::to_string(&json!(["EVENT", &event_json])).unwrap_or_default();

    Some(OutboundMessage {
        role: RelayRole::Wallet,
        relay_url: relay_url.to_string(),
        text,
    })
}

/// Sign a kind:23194 event with the NWC client secret key.
///
/// `created_at_secs` is supplied by the caller (`build_request`), which reads
/// it from the kernel's injected [`Clock`] (D9: the kernel owns time). Keeping
/// this crypto helper free of a `Kernel` dependency means the timestamp source
/// is the caller's concern and stays `FixedClock`-testable.
fn sign_nwc_request(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    encrypted_content: &str,
    created_at_secs: u64,
) -> Result<SignedEvent, String> {
    let sk = SecretKey::from_hex(client_secret_hex).map_err(|e| format!("client secret: {e}"))?;
    let wallet_pk =
        PublicKey::from_hex(wallet_pubkey_hex).map_err(|e| format!("wallet pubkey: {e}"))?;
    let keys = Keys::new(sk);
    let p_tag = Tag::public_key(wallet_pk);
    let created_at = Timestamp::from(created_at_secs);
    let event = EventBuilder::new(Kind::from_u16(23194), encrypted_content)
        .tags([p_tag])
        .custom_created_at(created_at)
        .sign_with_keys(&keys)
        .map_err(|e| format!("sign: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: crate::substrate::UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: 23194,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Serialize a `SignedEvent` into the Nostr EVENT JSON object.
fn build_event_json(signed: &SignedEvent) -> serde_json::Value {
    json!({
        "id": signed.id,
        "pubkey": signed.unsigned.pubkey,
        "created_at": signed.unsigned.created_at,
        "kind": signed.unsigned.kind,
        "tags": signed.unsigned.tags,
        "content": signed.unsigned.content,
        "sig": signed.sig,
    })
}

/// Push current wallet state to the shared status slot (D4: actor is sole
/// writer). The `"wallet"` snapshot projection reads this slot on the next
/// tick; a poisoned mutex is a silent no-op (D6 — a wallet write never panics
/// the actor thread).
///
/// Also marks the kernel dirty so the next due tick actually emits. The wallet
/// status is NOT a kernel field (D0 — NWC is an app noun), so writing the slot
/// alone would not flip `changed_since_emit`; without this a kind:23195
/// balance response — which the kernel drops as an unknown kind — could sit
/// unprojected until some unrelated kernel mutation triggers an emit.
fn sync_wallet_status(wallet: &WalletRuntime, kernel: &mut Kernel) {
    let status = wallet.connection.as_ref().map(|c| WalletStatus {
        status: c.status.clone(),
        relay_url: c.relay_url.clone(),
        wallet_npub: c.wallet_npub.clone(),
        balance_msats: c.balance_msats,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    /// Regression guard: `sync_wallet_status` must mark the kernel dirty.
    ///
    /// Wallet status is NOT a kernel field (D0 — NWC is an app noun), so the
    /// slot write alone does not flip `changed_since_emit`. The actor's regular
    /// tick (`tick::flush_due`) only emits when that flag is set; without the
    /// explicit `mark_changed_since_emit`, a kind:23195 balance response — which
    /// the kernel itself drops as an unknown kind — would never drive a
    /// projection refresh until some unrelated kernel mutation happened to set
    /// the flag.
    #[test]
    fn sync_wallet_status_marks_kernel_dirty_so_the_projection_emits() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Clear the flag a fresh kernel starts with so the assertion below
        // genuinely observes `sync_wallet_status`'s effect.
        let _ = kernel.make_update(true);
        assert!(
            !kernel.changed_since_emit(),
            "precondition: a just-emitted kernel must be clean",
        );

        let wallet = WalletRuntime::new(new_wallet_status_slot());
        sync_wallet_status(&wallet, &mut kernel);

        assert!(
            kernel.changed_since_emit(),
            "sync_wallet_status must mark the kernel dirty so the next due \
             tick emits the refreshed wallet projection",
        );
    }

    // ── pay_invoice response round-trip ─────────────────────────────────────
    //
    // These tests cover the original bug: a kind:23195 `pay_invoice` response
    // was silently dropped. Every test below threads a dispatched
    // correlation_id all the way from `wallet_pay_invoice` through a
    // synthetic relay frame back into `action_results`, proving the round-trip
    // closes the host spinner that used to hang forever.

    /// The two endpoint keys for tests — a deterministic NWC connection.
    /// `CLIENT_SECRET` is what the host's NWC URI carries; `WALLET_SECRET` is
    /// the wallet service's side. Mirrors `decode.rs`'s test constants so the
    /// `nmp-nwc` crate's own round-trip tests stay readable alongside this
    /// integration layer.
    const TEST_CLIENT_SECRET: &str =
        "0101010101010101010101010101010101010101010101010101010101010101";
    const TEST_WALLET_SECRET: &str =
        "0202020202020202020202020202020202020202020202020202020202020202";

    fn wallet_pubkey_hex() -> String {
        nmp_nwc::crypto::client_pubkey_hex(TEST_WALLET_SECRET).unwrap()
    }

    /// Build a `nostr+walletconnect://` URI for the deterministic test keys.
    fn test_nwc_uri() -> String {
        format!(
            "nostr+walletconnect://{}?relay=wss%3A%2F%2Frelay.test&secret={}",
            wallet_pubkey_hex(),
            TEST_CLIENT_SECRET,
        )
    }

    /// Build a realistic `["EVENT", <sub>, {<event>}]` kind:23195 frame whose
    /// `content` is the NIP-04-encrypted `response_payload`, encrypted
    /// wallet→client.
    ///
    /// `request_event_id` is the id of the original kind:23194 request — it
    /// goes into the response's `e` tag, which is how `handle_nwc_text`
    /// correlates the reply to its inflight payment (NIP-47 §3.2).
    fn build_response_frame(
        response_event_id: &str,
        request_event_id: &str,
        response_payload: serde_json::Value,
    ) -> String {
        let wallet_pk = wallet_pubkey_hex();
        let client_pk = nmp_nwc::crypto::client_pubkey_hex(TEST_CLIENT_SECRET).unwrap();
        let plaintext = serde_json::to_string(&response_payload).unwrap();
        // Wallet encrypts to the client's pubkey using the wallet secret —
        // the same direction the real wallet service does.
        let content =
            nmp_nwc::crypto::encrypt(TEST_WALLET_SECRET, &client_pk, &plaintext).unwrap();
        let frame = json!([
            "EVENT",
            "sub-test",
            {
                "id": response_event_id,
                "kind": 23195u32,
                "pubkey": wallet_pk,
                "content": content,
                "tags": [["e", request_event_id]],
            }
        ]);
        serde_json::to_string(&frame).unwrap()
    }

    /// Drive `wallet_connect` then send a `get_info` response so the
    /// connection reaches `status = "ready"`. Returns the populated wallet
    /// runtime ready for a `wallet_pay_invoice` call.
    ///
    /// `get_info` responses don't carry the same correlation-id contract as
    /// `pay_invoice` (the request id is the connect-time bootstrap, not a
    /// dispatched action), so an arbitrary placeholder request id is enough
    /// — the handler matches on `result_type == "get_info"`, not the id.
    fn ready_wallet_for_payment(kernel: &mut Kernel) -> WalletRuntime {
        let mut wallet = WalletRuntime::new(new_wallet_status_slot());
        let _ = wallet_connect(&mut wallet, kernel, &test_nwc_uri());
        // Bring status to "ready". The actual `get_info` request id the
        // wallet would echo back via `e` is internal to wallet_connect's
        // outbound EVENT; for a single get_info we don't need to match it —
        // the response handler keys "ready" off `result_type`, not the id.
        let frame = build_response_frame(
            "ff".repeat(32).as_str(),
            "00".repeat(32).as_str(),
            json!({ "result_type": "get_info", "error": null, "result": {
                "alias": "test-wallet",
                "color": null, "pubkey": null, "network": null, "methods": ["pay_invoice"]
            } }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, kernel);
        wallet
    }

    /// Extract the kind:23194 event id from the first outbound EVENT frame
    /// `wallet_pay_invoice` produced — that is the request id the kind:23195
    /// response's `e` tag must carry so the response handler can correlate.
    fn first_pay_invoice_request_id(outbound: &[OutboundMessage]) -> String {
        let frame = outbound
            .iter()
            .find(|m| m.text.starts_with("[\"EVENT\""))
            .expect("wallet_pay_invoice must emit a kind:23194 EVENT frame");
        let parsed: serde_json::Value = serde_json::from_str(&frame.text).unwrap();
        parsed
            .as_array()
            .and_then(|arr| arr.get(1))
            .and_then(|ev| ev.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .expect("outbound EVENT must have an id")
    }

    /// Read `projections.action_results` from a fresh wire snapshot. Returns
    /// `Null` when the projection key is absent (nothing settled this tick).
    fn action_results_snapshot(kernel: &mut Kernel) -> serde_json::Value {
        let snapshot_json = kernel.make_update(true);
        let parsed: serde_json::Value = serde_json::from_str(&snapshot_json).unwrap();
        parsed
            .get("projections")
            .and_then(|v| v.get("action_results"))
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    /// A successful `pay_invoice` response carrying a dispatched correlation_id
    /// surfaces a terminal `"ok"` entry in `action_results` — the host's
    /// payment spinner clears on the next tick. This is the round-trip the
    /// original bug broke: before the fix, the response was decoded but the
    /// `pay_invoice` branch did nothing, so the spinner hung forever.
    #[test]
    fn pay_invoice_response_success_surfaces_ok_terminal_in_action_results() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = ready_wallet_for_payment(&mut kernel);
        // Drain any action_results that may have been incidentally produced
        // by the bootstrap.
        let _ = action_results_snapshot(&mut kernel);

        let correlation_id = "corr-pay-ok".to_string();
        let outbound = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc100n1p3xnhl2pp5",
            Some(10_000),
            Some(correlation_id.clone()),
        );
        let request_id = first_pay_invoice_request_id(&outbound);

        // Synthesize the wallet's successful pay_invoice response. The `e`
        // tag MUST carry `request_id` — that's how `handle_nwc_text` finds
        // the inflight payment.
        let frame = build_response_frame(
            "ee".repeat(32).as_str(),
            &request_id,
            json!({
                "result_type": "pay_invoice",
                "error": null,
                "result": { "preimage": "deadbeef".repeat(8) }
            }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

        let results = action_results_snapshot(&mut kernel);
        let arr = results
            .as_array()
            .expect("a settled pay_invoice must surface a terminal in action_results");
        let entry = arr
            .iter()
            .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
            .expect("the dispatch correlation_id must appear in action_results");
        // The wire-side serializer (`take_action_results_projection`)
        // translates engine status `"ok"` → host-visible `"published"`. The
        // iOS shell keys its spinner cleanup on this exact string.
        assert_eq!(
            entry.get("status").and_then(|v| v.as_str()),
            Some("published"),
            "successful pay_invoice must report the wire status `published`",
        );
        assert!(
            entry.get("error").map(|v| v.is_null()).unwrap_or(true),
            "success entry must carry null/absent error",
        );
    }

    /// A `pay_invoice` response carrying an `error` object closes the dispatched
    /// correlation_id with a `"failed"` terminal — the host sees the actual
    /// NWC error code instead of a generic timeout, and the spinner clears.
    #[test]
    fn pay_invoice_response_error_surfaces_failed_terminal_in_action_results() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = ready_wallet_for_payment(&mut kernel);
        let _ = action_results_snapshot(&mut kernel);

        let correlation_id = "corr-pay-err".to_string();
        let outbound = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc200n1xxx",
            Some(20_000),
            Some(correlation_id.clone()),
        );
        let request_id = first_pay_invoice_request_id(&outbound);

        let frame = build_response_frame(
            "11".repeat(32).as_str(),
            &request_id,
            json!({
                "result_type": "pay_invoice",
                "error": { "code": "PAYMENT_FAILED", "message": "no route" },
                "result": null
            }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

        let results = action_results_snapshot(&mut kernel);
        let arr = results
            .as_array()
            .expect("a failed pay_invoice must surface a terminal in action_results");
        let entry = arr
            .iter()
            .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
            .expect("the dispatch correlation_id must appear in action_results");
        assert_eq!(
            entry.get("status").and_then(|v| v.as_str()),
            Some("failed"),
            "an error response reports the terminal `failed` status",
        );
        let err = entry
            .get("error")
            .and_then(|v| v.as_str())
            .expect("a failed entry carries a non-null error string");
        assert!(
            err.contains("PAYMENT_FAILED") && err.contains("no route"),
            "the failure carries the NWC code + message verbatim: {err}",
        );
    }

    /// A C-ABI direct caller passes `correlation_id == None`. The response
    /// handler must NOT panic, MUST still drain the per-payment entry, and
    /// MUST NOT push any spurious entry into `action_results` (nothing is
    /// waiting on an id). Without this path the no-correlation case would
    /// hit the same bug that motivated the fix.
    #[test]
    fn pay_invoice_response_without_correlation_id_drains_silently() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = ready_wallet_for_payment(&mut kernel);
        let _ = action_results_snapshot(&mut kernel);

        let outbound = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc300n1xxx",
            None, // No amount override.
            None, // No dispatched correlation_id — C-ABI direct path.
        );
        let request_id = first_pay_invoice_request_id(&outbound);

        let frame = build_response_frame(
            "22".repeat(32).as_str(),
            &request_id,
            json!({
                "result_type": "pay_invoice",
                "error": null,
                "result": { "preimage": "cafe".repeat(16) }
            }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

        let results = action_results_snapshot(&mut kernel);
        assert!(
            results.is_null() || results.as_array().map(Vec::is_empty).unwrap_or(false),
            "C-ABI direct pay_invoice must NOT push an action_results entry (got {results})",
        );
        // The pending_payments slot must have been removed — verify via the
        // private field through the connection accessor used by other tests.
        assert!(
            wallet
                .connection
                .as_ref()
                .map(|c| !c.pending_payments.contains_key(&request_id))
                .unwrap_or(true),
            "the response handler must drain pending_payments even when correlation_id is None",
        );
    }

    /// A dispatched payment whose wallet response carries an unmatched `e`
    /// tag (a stale or duplicate frame the connection didn't initiate) MUST
    /// NOT push an `action_results` entry under any other inflight
    /// correlation_id — the dispatched action's spinner remains waiting for
    /// its own response. D6: silent on unknown, never panic.
    #[test]
    fn pay_invoice_response_with_unknown_request_id_does_not_misroute() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = ready_wallet_for_payment(&mut kernel);
        let _ = action_results_snapshot(&mut kernel);

        // Issue a real payment so an inflight entry exists.
        let correlation_id = "corr-still-waiting".to_string();
        let _outbound = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc400n1xxx",
            Some(40_000),
            Some(correlation_id.clone()),
        );

        // Send a response whose `e` tag points to a request id we never
        // sent.
        let bogus_request_id = "ab".repeat(32);
        let frame = build_response_frame(
            "33".repeat(32).as_str(),
            &bogus_request_id,
            json!({
                "result_type": "pay_invoice",
                "error": null,
                "result": { "preimage": "00".repeat(32) }
            }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);

        let results = action_results_snapshot(&mut kernel);
        if let Some(arr) = results.as_array() {
            assert!(
                arr.iter().all(|e| {
                    e.get("correlation_id").and_then(|v| v.as_str()) != Some(&correlation_id)
                }),
                "an unmatched response must not falsely close the unrelated inflight payment",
            );
        }
    }

    /// Build an `["EVENT", <sub>, {event}]` kind:23195 frame WITHOUT any
    /// `tags` field — exercises the bootstrap-compatibility path where a
    /// real-world wallet (some Alby / Mutiny builds) returns a `get_info`
    /// reply that doesn't carry the NIP-47 §3.2 `e` tag. The lenient decoder
    /// must accept it; only `pay_invoice` correlation needs the tag.
    fn build_response_frame_no_tags(
        response_event_id: &str,
        response_payload: serde_json::Value,
    ) -> String {
        let wallet_pk = wallet_pubkey_hex();
        let client_pk = nmp_nwc::crypto::client_pubkey_hex(TEST_CLIENT_SECRET).unwrap();
        let plaintext = serde_json::to_string(&response_payload).unwrap();
        let content =
            nmp_nwc::crypto::encrypt(TEST_WALLET_SECRET, &client_pk, &plaintext).unwrap();
        let frame = json!([
            "EVENT",
            "sub-test",
            {
                "id": response_event_id,
                "kind": 23195u32,
                "pubkey": wallet_pk,
                "content": content,
                // No `tags` field — some wallets omit it on get_info.
            }
        ]);
        serde_json::to_string(&frame).unwrap()
    }

    /// Bootstrap-compatibility regression guard: a `get_info` response WITHOUT
    /// an `e` tag must still drive the connection to `status = "ready"`.
    /// Tightening the response handler to the strict NIP-47 §3.2 decoder for
    /// bootstrap would break real shipped wallets that omit the tag — the
    /// strict matcher is only applied to `pay_invoice` (where the
    /// correlation IS protocol-mandatory).
    #[test]
    fn get_info_response_without_e_tag_still_drives_status_ready() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = WalletRuntime::new(new_wallet_status_slot());
        let _ = wallet_connect(&mut wallet, &mut kernel, &test_nwc_uri());
        let frame = build_response_frame_no_tags(
            "aa".repeat(32).as_str(),
            json!({ "result_type": "get_info", "error": null, "result": {
                "alias": "lenient-wallet",
                "color": null, "pubkey": null, "network": null, "methods": ["pay_invoice"]
            } }),
        );
        let _ = handle_nwc_text(&mut wallet, &frame, &mut kernel);
        assert_eq!(
            wallet.connection.as_ref().map(|c| c.status.as_str()),
            Some("ready"),
            "a get_info response without the `e` tag must still bring the wallet to `ready`",
        );
    }

    /// `WalletDisconnect` mid-payment must close every inflight dispatched
    /// correlation_id as `Failed` — without this fix a user who cancels a
    /// payment (or whose iOS shell tears down the connection on backgrounding)
    /// leaks the host spinner exactly the same way the response-not-handled
    /// bug did. Same broken-promise class, different lifecycle entry point.
    #[test]
    fn wallet_disconnect_closes_inflight_pay_invoice_correlations() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = ready_wallet_for_payment(&mut kernel);
        let _ = action_results_snapshot(&mut kernel);

        let cid_a = "corr-inflight-a".to_string();
        let cid_b = "corr-inflight-b".to_string();
        let _ = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc100n1aaa",
            Some(10_000),
            Some(cid_a.clone()),
        );
        let _ = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc200n1bbb",
            Some(20_000),
            Some(cid_b.clone()),
        );

        // User backgrounds the app / cancels — the iOS shell calls
        // `nmp_app_wallet_disconnect`, which routes through here.
        let _ = wallet_disconnect(&mut wallet, &mut kernel);

        let results = action_results_snapshot(&mut kernel);
        let arr = results
            .as_array()
            .expect("disconnect must produce action_results terminals for inflight payments");
        let ids: std::collections::HashSet<&str> = arr
            .iter()
            .filter_map(|e| e.get("correlation_id").and_then(|v| v.as_str()))
            .collect();
        assert!(
            ids.contains(cid_a.as_str()) && ids.contains(cid_b.as_str()),
            "both inflight correlation_ids must close as Failed on disconnect (got {ids:?})",
        );
        for entry in arr {
            if let Some(cid) = entry.get("correlation_id").and_then(|v| v.as_str()) {
                if cid == cid_a || cid == cid_b {
                    assert_eq!(
                        entry.get("status").and_then(|v| v.as_str()),
                        Some("failed"),
                        "disconnect-induced termination reports `failed`",
                    );
                }
            }
        }
    }

    /// A dispatched `pay_invoice` called against a wallet that never
    /// connected (or whose status is still "connecting") fails the action
    /// CLOSED — the host spinner clears immediately rather than waiting on a
    /// kind:23195 response that will never come (because no kind:23194 went
    /// out). Mirrors the sign-step early-exit precedent in `publish.rs`.
    #[test]
    fn pay_invoice_with_no_connected_wallet_records_immediate_failure() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut wallet = WalletRuntime::new(new_wallet_status_slot());
        let _ = action_results_snapshot(&mut kernel);

        let correlation_id = "corr-no-wallet".to_string();
        let outbound = wallet_pay_invoice(
            &mut wallet,
            &mut kernel,
            "lnbc500n1xxx",
            Some(50_000),
            Some(correlation_id.clone()),
        );
        assert!(
            outbound.is_empty(),
            "no wallet means no outbound — request never goes on the wire",
        );

        let results = action_results_snapshot(&mut kernel);
        let arr = results
            .as_array()
            .expect("the early-exit failure must surface an action_results terminal");
        let entry = arr
            .iter()
            .find(|e| e.get("correlation_id").and_then(|v| v.as_str()) == Some(&correlation_id))
            .expect("dispatched correlation_id must be closed even on the early-exit path");
        assert_eq!(
            entry.get("status").and_then(|v| v.as_str()),
            Some("failed"),
            "no-wallet early exit reports the terminal `failed` status",
        );
    }
}
