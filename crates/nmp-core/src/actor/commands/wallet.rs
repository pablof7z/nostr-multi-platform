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
use nmp_nwc::decode::try_decode_relay_message_with_id;
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
    /// Inflight NWC requests: event_id → method name.
    pending: HashMap<String, String>,
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

    // Send get_info and get_balance immediately.
    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::GetInfo, json!({})) {
        out.push(msg);
    }
    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::GetBalance, json!({})) {
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
pub(crate) fn wallet_pay_invoice(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
    bolt11: &str,
    amount_msats: Option<u64>,
) -> Vec<OutboundMessage> {
    let conn = match &wallet.connection {
        Some(c) if c.status == "ready" => c,
        Some(_) => {
            kernel.set_last_error_toast(Some("wallet not ready — still connecting".to_string()));
            return Vec::new();
        }
        None => {
            kernel.set_last_error_toast(Some("no wallet connected".to_string()));
            return Vec::new();
        }
    };
    let relay = conn.relay_url.clone();
    let params = json!(PayInvoiceParams {
        invoice: bolt11.to_string(),
        amount: amount_msats,
    });
    if let Some(msg) = build_request(wallet, kernel, &relay, NwcMethod::PayInvoice, params) {
        return vec![msg];
    }
    Vec::new()
}

// ── Relay message intercept ───────────────────────────────────────────────────

/// Called from `handle_relay_event` when a message arrives from the NWC relay.
///
/// Parses kind:23195 EVENT frames, decrypts the content, and updates wallet state.
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

    let Some((_event_id, response)) = try_decode_relay_message_with_id(
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
fn build_request(
    wallet: &mut WalletRuntime,
    kernel: &mut Kernel,
    relay_url: &str,
    method: NwcMethod,
    params: serde_json::Value,
) -> Option<OutboundMessage> {
    let conn = wallet.connection.as_mut()?;

    let content = match nmp_nwc::build::request_content(
        conn.client_secret_hex.as_str(),
        &conn.wallet_pubkey_hex,
        &method,
        params,
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
}
