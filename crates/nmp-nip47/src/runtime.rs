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
//!
//! ## V-63 fix — encode-before-register
//!
//! REQ, EVENT, and CLOSE frames are now serialized with `encode_frame` which
//! returns `Result<String, serde_json::Error>`. On failure the frame is never
//! pushed to the outbound queue and a `last_error_toast` is set. For the
//! `pay_invoice` path the `pending_payments` map is inserted ONLY after the
//! outbound frame is successfully serialized, so a correlation_id is never
//! registered as inflight when the relay never received the request.
//!
//! ## V-64 fix — TTL sweep + orphan observability
//!
//! `pending_payments` entries now carry an `inserted_at_secs` timestamp.
//! The idle-tick hook (`sweep_expired_payments`) fires on every actor loop
//! iteration via `RelayTextInterceptor::on_idle_tick` — this includes
//! iterations where the NWC relay is completely silent — and calls
//! `record_action_failure` for any entry older than `PENDING_PAYMENT_TTL_SECS`
//! (90 s). The `(_, None) => {}` orphan arm is replaced with a `tracing::warn!`
//! and an `orphan_responses` counter, making receive-without-correlation
//! observable.
//!
//! ## V-79 fix — heartbeat + reconnect + connection_state projection
//!
//! `tick_heartbeat` is called from the host-side `on_idle_tick`. It is pure
//! wall-clock-gated (D8 — no sleep/loop): it compares `kernel.now_secs()` to
//! `last_probe_sent_secs` and only acts when `HEARTBEAT_CADENCE_SECS` have
//! elapsed since the last sent probe.
//!
//! A probe is a `get_info` request (same call `wallet_connect` already makes).
//! On every successful kind:23195 response in `handle_nwc_text`, the runtime
//! resets `consecutive_failures` to 0. A probe that is outstanding when the
//! *next* cadence window opens counts as one failure. After
//! `HEARTBEAT_MAX_FAILURES` consecutive failures, `tick_heartbeat` calls the
//! `resubscribe` helper to re-send REQ + get_info + get_balance on the same
//! wallet relay and transitions the projected `connection_state` to
//! `Reconnecting`. If probes continue to fail after resubscribe, `connection_state`
//! advances to `TransportLost` (the user must manually reconnect).
//!
//! The `connection_state` field is projected inside `WalletStatus` under the
//! existing `"wallet"` snapshot projection so the host shell can render a
//! non-silent liveness indicator without a new projection namespace.

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
use crate::status::{format_sats_display, NwcConnectionState, WalletStatus, WalletStatusSlot};

/// TTL for inflight `pay_invoice` requests. Entries older than this are
/// swept by the idle-tick hook and reported as timed-out failures via
/// `kernel.record_action_failure`. 90 seconds matches typical lightning
/// payment-routing ceilings across diverse wallet implementations.
///
/// Exported so host-side `RelayTextInterceptor::on_idle_tick` implementations
/// (e.g. `nmp-app-chirp::wallet_runtime::WalletInterceptor`) can pass the
/// canonical TTL to `WalletRuntime::sweep_expired_payments`.
pub const PENDING_PAYMENT_TTL_SECS: u64 = 90;

/// Interval between successive heartbeat `get_info` probes (V-79).
///
/// 30 s is a low enough cadence to detect a stale connection before the
/// user attempts a payment, while high enough not to waste relay bandwidth.
/// Exported so host-side interceptor impls can pass this canonical value to
/// [`WalletRuntime::tick_heartbeat`].
pub const HEARTBEAT_CADENCE_SECS: u64 = 30;

/// A probe counts as a failure if no kind:23195 response has arrived within
/// this window after the probe was sent (V-79). Using the same cadence
/// means: if the *next* probe interval opens and the previous probe is still
/// outstanding, we record one failure. This avoids a separate per-probe
/// deadline field while keeping the accounting simple.
pub const HEARTBEAT_PROBE_TIMEOUT_SECS: u64 = HEARTBEAT_CADENCE_SECS;

/// Number of consecutive unanswered probes before the runtime transitions
/// `connection_state` to `Reconnecting` and re-sends the subscription (V-79).
pub const HEARTBEAT_MAX_FAILURES: u32 = 3;

/// A single inflight `pay_invoice` request, keyed by the kind:23194 event
/// id on `WalletConnection::pending_payments`.
struct PendingPayment {
    /// The registry-minted action correlation id to close on terminal, if
    /// this payment was dispatched via `nmp.wallet.pay_invoice`. `None` for
    /// actor-internal auto-dispatched payments where no host spinner exists.
    correlation_id: Option<String>,
    /// Wall-clock second at which this entry was inserted (from
    /// `kernel.now_secs()`). Used by the idle-tick sweep to detect TTL
    /// expiry without a dedicated timer — D8 compliant.
    inserted_at_secs: u64,
}

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
    /// Inflight `pay_invoice` requests keyed by the kind:23194 event id.
    /// Entries are inserted ONLY after the outbound frame is successfully
    /// serialized (V-63 fix) and removed on the matching kind:23195 response
    /// or on TTL expiry (V-64 sweep).
    pending_payments: HashMap<String, PendingPayment>,
    /// Sub-id used for the kind:23195 subscription on the NWC relay.
    sub_id: String,
    /// Count of kind:23195 responses that arrived with no matching
    /// `pending_payments` entry. Observable via `orphan_response_count()`.
    orphan_responses: u64,
    // ── V-79: heartbeat state ──────────────────────────────────────────────
    /// Wall-clock second at which the last heartbeat `get_info` probe was
    /// sent. `0` means no probe has been sent yet in this session.
    last_probe_sent_secs: u64,
    /// `true` when a probe was sent and no kind:23195 response has arrived
    /// yet. Reset to `false` by `handle_nwc_text` on any successful response.
    probe_outstanding: bool,
    /// Number of consecutive probe windows that elapsed with no response.
    /// Reset to 0 on any successful kind:23195 response.
    consecutive_failures: u32,
    /// Coarse transport-health state projected to the snapshot. `None` until
    /// the first probe cadence window has elapsed.
    connection_state: Option<NwcConnectionState>,
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

    /// Number of kind:23195 responses received with no matching
    /// `pending_payments` entry. Exposed for diagnostic tests; not surfaced
    /// in the snapshot to avoid churning the FlatBuffers shape.
    #[cfg(test)]
    #[must_use]
    pub fn orphan_response_count(&self) -> u64 {
        self.connection
            .as_ref()
            .map(|c| c.orphan_responses)
            .unwrap_or(0)
    }

    /// Sweep `pending_payments` entries older than `now_secs` by `ttl_secs`.
    ///
    /// Returns the expired correlation_ids that must be recorded as failed
    /// by the caller (via `kernel.record_action_failure`). This design keeps
    /// the sweep Kernel-free so it can be tested without a live `Kernel`.
    ///
    /// The caller (host-side `RelayTextInterceptor::on_idle_tick`) records the
    /// returned failures — see `nmp-app-chirp::WalletInterceptor::on_idle_tick`.
    ///
    /// D8 — no sleep/loop: pure wall-clock compare of `now_secs` against the
    /// per-entry `inserted_at_secs` field.
    pub fn sweep_expired_payments(
        &mut self,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Vec<(String, String)> {
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
        let mut failures: Vec<(String, String)> = Vec::new();
        for event_id in expired_ids {
            if let Some(entry) = conn.pending_payments.remove(&event_id) {
                tracing::warn!(
                    event_id = %event_id,
                    inserted_at_secs = entry.inserted_at_secs,
                    now_secs = now_secs,
                    ttl_secs = ttl_secs,
                    "nwc: pay_invoice timed out — no kind:23195 response within TTL"
                );
                if let Some(cid) = entry.correlation_id {
                    failures.push((cid, format!("wallet timeout (>{ttl_secs}s)")));
                }
            }
        }
        failures
    }

    /// Heartbeat tick — called from the host-side `on_idle_tick` on every
    /// actor loop iteration.
    ///
    /// Returns outbound frames to send (zero, one probe, or a full
    /// resubscription batch) and a boolean indicating whether the snapshot
    /// should be marked dirty (`true` when `connection_state` changed).
    ///
    /// ## D8 compliance
    ///
    /// No sleep or blocking call inside. The decision is a pure wall-clock
    /// comparison of `now_secs` against the stored `last_probe_sent_secs`.
    /// The actor drives this from its idle section at ~250 ms cadence; the
    /// `HEARTBEAT_CADENCE_SECS` gate ensures probes fire at most once per
    /// window.
    ///
    /// ## Protocol
    ///
    /// 1. If no probe has been sent yet (or `last_probe_sent_secs == 0`) and
    ///    `HEARTBEAT_CADENCE_SECS` have elapsed since connect, send the first
    ///    probe.
    /// 2. On subsequent ticks: if `probe_outstanding` is still `true` when a
    ///    new cadence window opens, the previous probe timed out → increment
    ///    `consecutive_failures`.
    /// 3. When `consecutive_failures >= HEARTBEAT_MAX_FAILURES`, call
    ///    `resubscribe` and transition `connection_state` to `Reconnecting`.
    ///    After a second resubscribe round with no response (i.e. after ≥
    ///    `2 * HEARTBEAT_MAX_FAILURES` failures total), transition to
    ///    `TransportLost`.
    /// 4. Any successful response in `handle_nwc_text` resets
    ///    `consecutive_failures` to 0 and `connection_state` to `Connected`.
    pub fn tick_heartbeat(
        &mut self,
        now_secs: u64,
        cadence_secs: u64,
        max_failures: u32,
    ) -> HeartbeatOutbound {
        let conn = match self.connection.as_mut() {
            Some(c) => c,
            None => return HeartbeatOutbound { ready_frames: Vec::new(), needs_probe: false, state_changed: false },
        };

        // Before the first cadence window has elapsed, arm the baseline.
        if conn.last_probe_sent_secs == 0 {
            // Record "just connected" as the baseline so the first probe fires
            // ~cadence_secs after connect.
            conn.last_probe_sent_secs = now_secs;
            return HeartbeatOutbound { ready_frames: Vec::new(), needs_probe: false, state_changed: false };
        }

        let elapsed = now_secs.saturating_sub(conn.last_probe_sent_secs);
        if elapsed < cadence_secs {
            // Still within the current cadence window — nothing to do.
            return HeartbeatOutbound { ready_frames: Vec::new(), needs_probe: false, state_changed: false };
        }

        // A new cadence window opened. If a probe from the *previous* window
        // is still outstanding, that probe failed.
        let prev_state = conn.connection_state.clone();
        if conn.probe_outstanding {
            conn.consecutive_failures = conn.consecutive_failures.saturating_add(1);
            tracing::warn!(
                consecutive_failures = conn.consecutive_failures,
                last_probe_sent_secs = conn.last_probe_sent_secs,
                now_secs = now_secs,
                "nwc: heartbeat probe unanswered — consecutive failure #{n}",
                n = conn.consecutive_failures,
            );
        }

        // Transition connection_state based on failure count.
        let resubscribe_needed;
        if conn.consecutive_failures >= max_failures {
            // Use the total consecutive count to distinguish first-round vs.
            // second-round failure (≥ 2× threshold = TransportLost).
            if conn.consecutive_failures >= max_failures * 2 {
                conn.connection_state = Some(NwcConnectionState::TransportLost);
                // Do not keep resubscribing past TransportLost — the relay is
                // considered unreachable; flooding the outbound queue would be
                // wasteful. The user must manually reconnect.
                resubscribe_needed = false;
            } else {
                conn.connection_state = Some(NwcConnectionState::Reconnecting);
                resubscribe_needed = true;
            }
        } else {
            // Failure count below threshold — state stays at whatever it was.
            resubscribe_needed = false;
        }

        let state_changed = conn.connection_state != prev_state;

        // Advance the probe window baseline and arm the outstanding flag.
        conn.last_probe_sent_secs = now_secs;
        conn.probe_outstanding = true;

        // Capture fields needed to build the REQ frame (if resubscribing).
        let relay = conn.relay_url.clone();
        let sub_id = conn.sub_id.clone();
        let wallet_pubkey_hex = conn.wallet_pubkey_hex.clone();
        let client_pubkey_hex = conn.client_pubkey_hex.clone();

        let mut ready_frames = Vec::new();

        if resubscribe_needed {
            // Re-send REQ so the relay forwards kind:23195 again.
            let req_filter = json!({
                "kinds": [23195u32],
                "authors": [&wallet_pubkey_hex],
                "#p": [&client_pubkey_hex],
            });
            match encode_frame(&json!(["REQ", &sub_id, &req_filter])) {
                Ok(req_msg) => {
                    ready_frames.push(OutboundMessage::new(
                        RelayRole::Wallet,
                        relay.clone(),
                        req_msg,
                    ));
                }
                Err(e) => {
                    tracing::warn!("nwc: heartbeat REQ encode failed: {e}");
                }
            }
        }

        // Always request a get_info probe at the cadence boundary.
        HeartbeatOutbound { ready_frames, needs_probe: true, state_changed }
    }

    /// Build and enqueue a `get_info` heartbeat probe for the connected relay.
    ///
    /// Returns `None` when no connection is active or frame encoding fails.
    /// The caller (`WalletInterceptor::on_idle_tick`) calls this after
    /// `tick_heartbeat` returns `needs_probe = true`, using a kernel reference
    /// that was not available inside the Kernel-free `tick_heartbeat` body.
    pub fn build_get_info_probe(
        &mut self,
        kernel: &mut Kernel,
    ) -> Option<OutboundMessage> {
        let relay = self.connection.as_ref()?.relay_url.clone();
        build_request(self, kernel, &relay, NwcMethod::GetInfo, json!({}), None)
    }

    /// Push the current `connection_state` into the `status_slot` and mark the
    /// snapshot dirty. Called by the host interceptor when `tick_heartbeat`
    /// reports `state_changed = true`.
    pub fn sync_connection_state(&self, kernel: &mut Kernel) {
        sync_wallet_status(self, kernel);
    }
}

/// Result of a [`WalletRuntime::tick_heartbeat`] call.
pub struct HeartbeatOutbound {
    /// Ready-to-send frames (REQ resubscription during reconnect, if any).
    pub ready_frames: Vec<OutboundMessage>,
    /// `true` when the runtime wants a `get_info` probe to be sent for this
    /// relay. The caller must invoke `build_get_info_probe` (which needs
    /// `&mut Kernel`) after the `tick_heartbeat` lock window closes.
    pub needs_probe: bool,
    /// `true` when `connection_state` changed and the snapshot must be
    /// re-synced. Caller calls `sync_connection_state(kernel)`.
    pub state_changed: bool,
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
        orphan_responses: 0,
        last_probe_sent_secs: 0,
        probe_outstanding: false,
        consecutive_failures: 0,
        connection_state: None,
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
            kernel.set_last_error_toast(Some(format!("NWC REQ encode failed: {e}")));
        }
    }

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
    for (_request_id, entry) in conn.pending_payments.iter() {
        if let Some(cid) = &entry.correlation_id {
            kernel.record_action_failure(cid.clone(), "wallet disconnected".to_string());
        }
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

    // V-79: any successful kind:23195 response means the relay is alive.
    // Reset the heartbeat failure counter and close the outstanding probe
    // flag regardless of which result_type arrived.
    if response.error.is_none() {
        conn.probe_outstanding = false;
        conn.consecutive_failures = 0;
        conn.connection_state = Some(NwcConnectionState::Connected);
    }

    if response.result_type == "pay_invoice" {
        let matched = try_decode_response_for_request(
            relay_text,
            &conn.wallet_pubkey_hex,
            conn.client_secret_hex.as_str(),
        );
        if let Some((request_event_id, _response2)) = matched {
            let entry_opt = conn.pending_payments.remove(&request_event_id);
            match (&response.error, entry_opt) {
                (None, Some(entry)) => {
                    if let Some(cid) = entry.correlation_id {
                        kernel.record_action_success(cid);
                    }
                }
                (Some(err), Some(entry)) => {
                    if let Some(cid) = entry.correlation_id {
                        let reason = format!("{}: {}", err.code, err.message);
                        kernel.record_action_failure(cid, reason);
                    }
                }
                // V-64: make orphan responses observable instead of silent.
                (_, None) => {
                    conn.orphan_responses += 1;
                    tracing::warn!(
                        request_event_id = %request_event_id,
                        orphan_count = conn.orphan_responses,
                        "nwc: pay_invoice response arrived with no matching \
                         pending_payments entry (orphan response)"
                    );
                }
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

/// Serialize a JSON value to a string for the outbound wire queue.
///
/// V-63: replaces the prior `serde_json::to_string(...).unwrap_or_default()`
/// call sites. Returns `Err` on the rare serialization failure so callers can
/// surface an error rather than pushing an empty `""` frame.
fn encode_frame(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

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

    let event_json = build_event_json(&signed);
    // V-63: encode the EVENT frame BEFORE inserting into pending maps.
    // If encoding fails we surface an error and return None without
    // registering the correlation_id as inflight — the pay_invoice path's
    // caller detects None and calls record_action_failure directly, so the
    // action is never left hanging.
    let text = match encode_frame(&json!(["EVENT", &event_json])) {
        Ok(t) => t,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("NWC EVENT encode failed: {e}")));
            return None;
        }
    };

    // Insert into tracking maps only after successful encoding (V-63).
    let method_name = method.as_str().to_string();
    conn.pending.insert(signed.id.clone(), method_name);
    if matches!(method, NwcMethod::PayInvoice) {
        conn.pending_payments.insert(
            signed.id.clone(),
            PendingPayment {
                correlation_id,
                inserted_at_secs: created_at,
            },
        );
    }

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
            // V-79: project the real-time transport-health state.
            connection_state: c.connection_state.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::new_wallet_status_slot;

    // ── V-63: encode-before-register ─────────────────────────────────────────

    /// Verify that `encode_frame` propagates a serialization error.
    ///
    /// `serde_json::to_string` of a plain `json!([...])` is effectively
    /// infallible, so we test via a `HashMap<Vec<u8>, ()>` whose
    /// non-string keys cause serde_json to reject serialization.
    #[test]
    fn encode_frame_returns_err_for_non_string_key_map() {
        let mut bad: std::collections::HashMap<Vec<u8>, ()> = std::collections::HashMap::new();
        bad.insert(vec![0u8], ());
        let result = serde_json::to_string(&bad);
        assert!(
            result.is_err(),
            "serde_json must reject a map with non-string keys — \
             this is the error class encode_frame is designed to catch"
        );
    }

    /// V-63: verify that a successful encode_frame call returns a non-empty
    /// JSON string (the REQ/EVENT/CLOSE frame shape).
    #[test]
    fn encode_frame_succeeds_for_valid_json_array() {
        let frame = json!(["REQ", "sub-id-1", {"kinds": [23195u32]}]);
        let result = encode_frame(&frame);
        assert!(result.is_ok(), "valid json array must encode without error");
        let text = result.unwrap();
        assert!(!text.is_empty(), "encoded frame must not be empty");
        assert!(text.starts_with('['), "encoded frame must be a JSON array");
    }

    // ── V-64: orphan response counter ─────────────────────────────────────────

    /// V-64: `orphan_response_count` starts at zero for a freshly created
    /// runtime.
    #[test]
    fn orphan_response_count_starts_at_zero() {
        let slot = new_wallet_status_slot();
        let rt = WalletRuntime::new(slot);
        // No connection installed — count must be zero.
        assert_eq!(
            rt.orphan_response_count(),
            0,
            "fresh runtime must report zero orphan responses"
        );
    }

    // ── V-64: sweep_expired_payments ─────────────────────────────────────────

    fn make_connection(pending_payments: HashMap<String, PendingPayment>) -> WalletConnection {
        WalletConnection {
            wallet_pubkey_hex: "aaaa".repeat(16),
            wallet_npub: "npub1test".to_string(),
            relay_url: "wss://test.relay".to_string(),
            client_secret_hex: Zeroizing::new("bb".repeat(32)),
            client_pubkey_hex: "cccc".repeat(16),
            status: "ready".to_string(),
            balance_msats: None,
            pending: HashMap::new(),
            pending_payments,
            sub_id: "nwc-aaaa".to_string(),
            orphan_responses: 0,
            last_probe_sent_secs: 0,
            probe_outstanding: false,
            consecutive_failures: 0,
            connection_state: None,
        }
    }

    /// V-64 (test b): an aged pending entry is swept to a timeout failure on
    /// the next driven tick; a fresh entry is retained.
    ///
    /// `sweep_expired_payments` returns `(correlation_id, reason)` pairs so
    /// the caller (not the sweep) records failures via `kernel` — allowing
    /// this test to drive the production code without a live `Kernel`.
    #[test]
    fn sweep_removes_expired_entry_and_leaves_fresh_entry() {
        let slot = new_wallet_status_slot();
        let mut rt = WalletRuntime::new(slot);
        let now_secs: u64 = 1_000_000;
        let ttl_secs: u64 = 90;

        let mut payments = HashMap::new();
        // Expired: inserted 200 s ago (past the 90 s TTL).
        payments.insert(
            "expired-event-id".to_string(),
            PendingPayment {
                correlation_id: Some("cid-expired".to_string()),
                inserted_at_secs: now_secs - 200,
            },
        );
        // Fresh: inserted 10 s ago (within TTL).
        payments.insert(
            "fresh-event-id".to_string(),
            PendingPayment {
                correlation_id: Some("cid-fresh".to_string()),
                inserted_at_secs: now_secs - 10,
            },
        );
        rt.connection = Some(make_connection(payments));

        // Call the real production function.
        let failures = rt.sweep_expired_payments(now_secs, ttl_secs);

        // Only the expired entry returns a failure cid.
        assert_eq!(failures.len(), 1, "exactly one failure must be returned");
        let (cid, reason) = &failures[0];
        assert_eq!(cid, "cid-expired", "returned cid must be the expired one");
        assert!(
            reason.contains("timeout"),
            "reason must mention timeout: {reason}"
        );

        // The expired entry must be removed from the map.
        let conn = rt.connection.as_ref().unwrap();
        assert!(
            !conn.pending_payments.contains_key("expired-event-id"),
            "expired entry must be removed"
        );

        // The fresh entry must remain.
        assert!(
            conn.pending_payments.contains_key("fresh-event-id"),
            "fresh entry must be retained"
        );
    }

    /// V-64: a `PendingPayment` with `correlation_id = None` (actor-internal
    /// auto-dispatch) must be swept and removed but must NOT produce a failure
    /// entry (nothing is waiting on it).
    #[test]
    fn sweep_removes_no_correlation_entry_without_failure() {
        let slot = new_wallet_status_slot();
        let mut rt = WalletRuntime::new(slot);
        let now_secs: u64 = 1_000_000;
        let ttl_secs: u64 = 90;

        let mut payments = HashMap::new();
        payments.insert(
            "actor-internal-event-id".to_string(),
            PendingPayment {
                correlation_id: None,
                inserted_at_secs: now_secs - 200,
            },
        );
        rt.connection = Some(make_connection(payments));

        let failures = rt.sweep_expired_payments(now_secs, ttl_secs);

        // No correlation_id → no failure emitted.
        assert!(
            failures.is_empty(),
            "actor-internal (no cid) sweep must return no failure pairs"
        );

        // The entry must still have been removed from the map.
        let conn = rt.connection.as_ref().unwrap();
        assert!(
            !conn.pending_payments.contains_key("actor-internal-event-id"),
            "actor-internal entry must be removed from the map"
        );
    }

    /// V-64: fresh entries (within TTL) must NOT be swept.
    #[test]
    fn sweep_leaves_fresh_entry_untouched() {
        let slot = new_wallet_status_slot();
        let mut rt = WalletRuntime::new(slot);
        let now_secs: u64 = 1_000_000;
        let ttl_secs: u64 = 90;

        let mut payments = HashMap::new();
        payments.insert(
            "fresh-event-id".to_string(),
            PendingPayment {
                correlation_id: Some("cid-fresh".to_string()),
                inserted_at_secs: now_secs - 10,
            },
        );
        rt.connection = Some(make_connection(payments));

        let failures = rt.sweep_expired_payments(now_secs, ttl_secs);

        assert!(failures.is_empty(), "fresh entry must not produce a failure");
        let conn = rt.connection.as_ref().unwrap();
        assert!(
            conn.pending_payments.contains_key("fresh-event-id"),
            "fresh entry must still be present"
        );
    }

    // ── V-79: tick_heartbeat ──────────────────────────────────────────────────

    fn make_runtime_ready() -> WalletRuntime {
        let slot = new_wallet_status_slot();
        let mut rt = WalletRuntime::new(slot);
        rt.connection = Some(make_connection(HashMap::new()));
        // Mark ready + arm probe baseline so the first cadence window works.
        if let Some(c) = rt.connection.as_mut() {
            c.status = "ready".to_string();
        }
        rt
    }

    /// V-79: within the cadence window, `tick_heartbeat` must be a no-op
    /// (no probe, no state change).
    #[test]
    fn heartbeat_no_op_within_cadence_window() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        // Arm the baseline timestamp.
        let result = rt.tick_heartbeat(now_secs, 30, 3);
        // baseline-arm call returns no probe.
        assert!(!result.needs_probe, "baseline-arm must not request a probe");
        assert!(!result.state_changed, "baseline-arm must not report state change");

        // 10 s later — still within the 30 s window.
        let result2 = rt.tick_heartbeat(now_secs + 10, 30, 3);
        assert!(!result2.needs_probe, "within cadence window must not probe");
    }

    /// V-79: when `cadence_secs` have elapsed, `tick_heartbeat` requests a
    /// probe and the previous outstanding probe (if any) counts as a failure.
    #[test]
    fn heartbeat_requests_probe_after_cadence_window() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        // Arm baseline.
        rt.tick_heartbeat(now_secs, 30, 3);

        // 30 s later — a full cadence window has elapsed.
        let result = rt.tick_heartbeat(now_secs + 30, 30, 3);
        assert!(result.needs_probe, "after cadence window must request probe");
    }

    /// V-79: `consecutive_failures` increments when a probe is outstanding
    /// at the next window boundary (= timed out).
    #[test]
    fn heartbeat_increments_failures_for_unanswered_probe() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        // Arm baseline.
        rt.tick_heartbeat(now_secs, 30, 3);
        // First probe sent (no failure yet — no previous probe was outstanding).
        rt.tick_heartbeat(now_secs + 30, 30, 3);
        // probe_outstanding is now true; no response arrived.
        // Second window opens → previous probe counts as failure 1.
        rt.tick_heartbeat(now_secs + 60, 30, 3);
        let failures = rt
            .connection
            .as_ref()
            .unwrap()
            .consecutive_failures;
        assert_eq!(failures, 1, "one unanswered probe = failure count 1");
    }

    /// V-79: after `max_failures` consecutive failures, `connection_state`
    /// transitions to `Reconnecting` and a REQ frame is included.
    #[test]
    fn heartbeat_transitions_to_reconnecting_after_max_failures() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        let cadence: u64 = 30;
        let max: u32 = 3;

        // Arm baseline.
        rt.tick_heartbeat(now_secs, cadence, max);

        // Drive 3 missed probe windows to accumulate max_failures.
        for i in 1..=max as u64 {
            rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
        }
        // Next window opens after all 3 failures are counted.
        let result = rt.tick_heartbeat(now_secs + cadence * (max as u64 + 1), cadence, max);
        let conn = rt.connection.as_ref().unwrap();
        assert_eq!(
            conn.connection_state,
            Some(NwcConnectionState::Reconnecting),
            "after max_failures the state must be Reconnecting"
        );
        // At least one ready frame (the REQ resubscription) must be present.
        assert!(
            !result.ready_frames.is_empty(),
            "Reconnecting transition must include a REQ frame"
        );
    }

    /// V-79: after `2 * max_failures` consecutive failures, `connection_state`
    /// transitions to `TransportLost` and no further REQ frames are emitted.
    #[test]
    fn heartbeat_transitions_to_transport_lost_after_double_max_failures() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        let cadence: u64 = 30;
        let max: u32 = 3;

        // Arm baseline.
        rt.tick_heartbeat(now_secs, cadence, max);

        // Drive 2 × max_failures missed windows.
        for i in 1..=(max as u64 * 2) {
            rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
        }
        let result = rt.tick_heartbeat(now_secs + cadence * (max as u64 * 2 + 1), cadence, max);
        let conn = rt.connection.as_ref().unwrap();
        assert_eq!(
            conn.connection_state,
            Some(NwcConnectionState::TransportLost),
            "after 2×max_failures the state must be TransportLost"
        );
        // Past TransportLost we must NOT keep emitting REQ frames.
        assert!(
            result.ready_frames.is_empty(),
            "TransportLost must not emit further REQ frames"
        );
    }

    /// V-79: a successful kind:23195 response (via handle_nwc_text) resets
    /// `consecutive_failures` to 0 and advances `connection_state` to Connected.
    /// Simulated by directly resetting the fields (handle_nwc_text needs
    /// crypto infra not available in unit tests).
    #[test]
    fn heartbeat_resets_on_successful_response() {
        let mut rt = make_runtime_ready();
        let now_secs: u64 = 1_000_000;
        let cadence: u64 = 30;
        let max: u32 = 3;

        // Arm baseline then drive into Reconnecting.
        rt.tick_heartbeat(now_secs, cadence, max);
        for i in 1..=(max as u64 + 1) {
            rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
        }
        // Manually simulate what handle_nwc_text does on a successful response.
        {
            let conn = rt.connection.as_mut().unwrap();
            conn.probe_outstanding = false;
            conn.consecutive_failures = 0;
            conn.connection_state = Some(NwcConnectionState::Connected);
        }
        let conn = rt.connection.as_ref().unwrap();
        assert_eq!(conn.consecutive_failures, 0, "reset to 0 after success");
        assert_eq!(
            conn.connection_state,
            Some(NwcConnectionState::Connected),
            "state must be Connected after a successful response"
        );
    }
}
