//! NIP-47 Nostr Wallet Connect actor-side runtime.
//!
//! Moved from `nmp-core::actor::commands::wallet` in V-38. The runtime lives
//! behind a [`WalletRuntimeHandle`] (`Arc<Mutex<Option<WalletRuntime>>>`).
//! Each wallet `ActionModule` value and the `WalletInterceptor` hold their own
//! `Arc` clone of the handle, obtained at composition time via
//! [`crate::register::register_wallet`] (ADR-0052 rung 5.2 — register-by-value,
//! no process-global install).
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
use nmp_core::substrate::{UnsignedEvent, WalletKernelAccess};
use nmp_core::{AuthSignerFn, OutboundMessage, RelayRole};
use nostr::nips::nip19::ToBech32;
use nostr::{Keys, PublicKey, SecretKey};
use serde_json::json;
use zeroize::Zeroizing;

use nmp_nwc::decode::{try_decode_relay_message_with_id, try_decode_response_for_request};
use nmp_nwc::parse::NwcUri;
use nmp_nwc::types::{LookupInvoiceParams, PayInvoiceParams};
use nmp_nwc::NwcMethod;

use crate::crypto::{build_event_json, sign_nwc_request, sign_with};
use crate::payment_store::{FsPaymentStore, PaymentRecord, PaymentState};
use crate::reconcile::{correct_unresolved_record, settle_payment_failure, settle_payment_success};
use crate::status::{NwcConnectionState, WalletStatus, WalletStatusSlot};

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
    /// The bolt11 invoice this payment is settling. Carried so the durable
    /// store record (and `lookup_invoice` reconciliation) can be written
    /// without re-deriving the invoice from the encrypted request content.
    bolt11: String,
    /// Amount in millisatoshis, if the dispatch carried an explicit amount.
    amount_msats: Option<u64>,
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
    /// In-flight `lookup_invoice` reconciliation requests, keyed by the
    /// kind:23194 lookup-request event id, mapping back to the ORIGINAL
    /// `pay_invoice` request event id whose outcome we are reconciling. The
    /// `lookup_invoice` kind:23195 reply's `e` tag points at the lookup
    /// request — this map bridges it back to the payment record.
    pending_lookups: HashMap<String, String>,
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
    /// Durable per-payment record store. `None` means in-memory-only (used in
    /// unit tests and pre-startup); `Some` activates the double-pay-safe
    /// write-before-enqueue + tri-state reconciliation path. The host installs
    /// it via [`WalletRuntime::set_payment_store`] using its storage path.
    payment_store: Option<FsPaymentStore>,
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

/// Construct a fresh, empty [`WalletRuntimeHandle`]. The host clones it into
/// (a) each wallet `ActionModule` value, (b) each `ProtocolCommand` those
/// modules construct, and (c) the relay-text interceptor — every consumer
/// carries the handle by value (ADR-0052 rung 5.2). No process-global slot.
#[must_use]
pub fn new_wallet_runtime_handle() -> WalletRuntimeHandle {
    Arc::new(Mutex::new(None))
}

// ADR-0052 rung 5.2: the process-global `ACTIVE_WALLET_RUNTIME`
// (`OnceLock<WalletRuntimeHandle>`) plus `install_wallet_runtime` /
// `active_wallet_runtime` were DELETED. The wallet runtime is now owned by
// value: each of the three wallet `ActionModule`s holds an
// `Arc<WalletRuntimeHandle>` captured at composition time, and the NIP-57 zap
// auto-chain carries the same handle through `FetchLnurlInvoiceCommand`. Two
// `NmpApp` instances in one process therefore drive fully independent wallet
// runtimes (proven by the `k2_two_instance_wallet_isolation` oracle), and a
// freed-then-recreated app re-initialises cleanly (no fired `OnceLock`).

impl WalletRuntime {
    /// Construct a wallet runtime bound to the shared status slot.
    #[must_use]
    pub fn new(status_slot: WalletStatusSlot) -> Self {
        Self {
            connection: None,
            status_slot,
            payment_store: None,
        }
    }

    /// Install the durable payment store. The host calls this once at
    /// construction using its storage path so in-flight payments survive a
    /// process kill and can be reconciled via `lookup_invoice` on reconnect.
    pub fn set_payment_store(&mut self, store: FsPaymentStore) {
        self.payment_store = Some(store);
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
        kernel: &dyn WalletKernelAccess,
    ) -> Option<OutboundMessage> {
        let relay = self.connection.as_ref()?.relay_url.clone();
        build_request(self, kernel, &relay, NwcMethod::GetInfo, json!({}), None).map(|(msg, _id)| msg)
    }

    /// Push the current `connection_state` into the `status_slot` and mark the
    /// snapshot dirty. Called by the host interceptor when `tick_heartbeat`
    /// reports `state_changed = true`.
    pub fn sync_connection_state(&self, kernel: &dyn WalletKernelAccess) {
        sync_wallet_status(self, kernel);
    }
}

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
        pending_lookups: HashMap::new(),
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
    if let Some((msg, _id)) =
        build_request(wallet, kernel, &relay, NwcMethod::GetBalance, json!({}), None)
    {
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

fn wallet_disconnect_inner(
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
                tracing::error!(
                    "nwc: status_slot lock was poisoned on disconnect — recovering"
                );
                poisoned.into_inner()
            }
        };
        let balance_sats = conn.balance_msats.map(|m| m / 1000);
        let wire = "disconnected";
        *slot = Some(WalletStatus {
            status: wire.to_string(),
            relay_url: conn.relay_url.clone(),
            wallet_npub: conn.wallet_npub.clone(),
            wallet_pubkey_hex: conn.wallet_pubkey_hex.clone(),
            balance_msats: conn.balance_msats,
            balance_sats,
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

/// Issue a `lookup_invoice` for every unresolved (`PaySent`/`Unknown`) durable
/// record so payments whose outcome we missed (TTL, disconnect, restart) are
/// reconciled. Returns the outbound `lookup_invoice` frames; registers each in
/// `pending_lookups` so the reply maps back to the original payment.
fn reconcile_unresolved_payments(
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
        if let Some((msg, lookup_request_id)) =
            build_request(wallet, kernel, relay, NwcMethod::LookupInvoice, params, None)
        {
            if let Some(conn) = wallet.connection.as_mut() {
                conn.pending_lookups
                    .insert(lookup_request_id, record.request_event_id.clone());
            }
            out.push(msg);
        }
    }
    out
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
fn build_request(
    wallet: &mut WalletRuntime,
    kernel: &dyn WalletKernelAccess,
    relay_url: &str,
    method: NwcMethod,
    params: serde_json::Value,
    correlation_id: Option<String>,
) -> Option<(OutboundMessage, String)> {
    build_request_with_meta(wallet, kernel, relay_url, method, params, correlation_id, None)
}

/// Metadata threaded into the `pay_invoice` tracking record.
struct PayMeta {
    bolt11: String,
    amount_msats: Option<u64>,
}

fn build_request_with_meta(
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

fn sync_wallet_status(wallet: &WalletRuntime, kernel: &dyn WalletKernelAccess) {
    let status = wallet.connection.as_ref().map(|c| {
        let balance_sats = c.balance_msats.map(|m| m / 1000);
        WalletStatus {
            status: c.status.clone(),
            relay_url: c.relay_url.clone(),
            wallet_npub: c.wallet_npub.clone(),
            wallet_pubkey_hex: c.wallet_pubkey_hex.clone(),
            balance_msats: c.balance_msats,
            balance_sats,
            wallet_npub_short: short_npub(&c.wallet_npub),
            is_ready: c.status == "ready",
            is_connected: c.status == "connecting" || c.status == "ready",
            // V-79: project the real-time transport-health state.
            connection_state: c.connection_state.clone(),
        }
    });
    // D6 poison-lock recovery: a panicking thread must not permanently brick
    // the status projection.  Recover the guard via `unwrap_or_else` so a
    // single actor panic never leaves the slot locked forever.  Log the
    // recovery so it is observable without crashing the actor thread.
    //
    // `mark_changed_since_emit` is called ONLY when the slot write succeeded.
    // Calling it after a skipped write would tell the snapshot machinery there
    // is new data to emit when in fact the slot still holds its prior value —
    // that is a stale-balance defect (D6: poison is not fatal, but we must not
    // lie about what we wrote).
    let mut slot = match wallet.status_slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!(
                "nwc: status_slot lock was poisoned — recovering; \
                 wallet projection may be temporarily stale"
            );
            poisoned.into_inner()
        }
    };
    *slot = status;
    drop(slot); // release before marking dirty
    kernel.mark_changed_since_emit();
}

fn pubkey_to_npub(hex: &str) -> Result<String, String> {
    PublicKey::from_hex(hex)
        .map_err(|e| format!("{e}"))?
        .to_bech32()
        .map_err(|e| format!("{e}"))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "runtime_money_path_tests.rs"]
mod money_path_tests;
