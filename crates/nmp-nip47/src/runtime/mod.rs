//! NIP-47 Nostr Wallet Connect actor-side runtime.
//!
//! Moved from `nmp-core::actor::commands::wallet` in V-38. The runtime lives
//! behind a [`WalletRuntimeHandle`] (`Arc<Mutex<Option<WalletRuntime>>>`).
//! Each wallet `ActionModule` value and the `WalletInterceptor` hold their own
//! `Arc` clone of the handle, obtained at composition time via
//! [`crate::register::register_wallet`] (ADR-0072 rung 5.2 — register-by-value,
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

use zeroize::Zeroizing;

use crate::payment_store::FsPaymentStore;
use crate::status::{NwcConnectionState, WalletStatusSlot};

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
pub(super) struct PendingPayment {
    /// The registry-minted action correlation id to close on terminal, if
    /// this payment was dispatched via `nmp.wallet.pay_invoice`. `None` for
    /// actor-internal auto-dispatched payments where no host spinner exists.
    pub(super) correlation_id: Option<String>,
    /// Wall-clock second at which this entry was inserted (from
    /// `kernel.now_secs()`). Used by the idle-tick sweep to detect TTL
    /// expiry without a dedicated timer — D8 compliant.
    pub(super) inserted_at_secs: u64,
    /// The bolt11 invoice this payment is settling. Carried so the durable
    /// store record (and `lookup_invoice` reconciliation) can be written
    /// without re-deriving the invoice from the encrypted request content.
    pub(super) bolt11: String,
    /// Amount in millisatoshis, if the dispatch carried an explicit amount.
    pub(super) amount_msats: Option<u64>,
}

/// Actor-local NWC connection state. Cleared on `wallet_disconnect`.
pub(super) struct WalletConnection {
    pub(super) wallet_pubkey_hex: String,
    pub(super) wallet_npub: String,
    pub(super) relay_url: String,
    pub(super) client_secret_hex: Zeroizing<String>,
    #[allow(dead_code)] // Retained for future per-event author filtering.
    pub(super) client_pubkey_hex: String,
    pub(super) status: String,
    pub(super) balance_msats: Option<u64>,
    /// Inflight NWC requests: event_id → method name. Diagnostic-only.
    pub(super) pending: HashMap<String, String>,
    /// Inflight `pay_invoice` requests keyed by the kind:23194 event id.
    /// Entries are inserted ONLY after the outbound frame is successfully
    /// serialized (V-63 fix) and removed on the matching kind:23195 response
    /// or on TTL expiry (V-64 sweep).
    pub(super) pending_payments: HashMap<String, PendingPayment>,
    /// In-flight `lookup_invoice` reconciliation requests, keyed by the
    /// kind:23194 lookup-request event id, mapping back to the ORIGINAL
    /// `pay_invoice` request event id whose outcome we are reconciling. The
    /// `lookup_invoice` kind:23195 reply's `e` tag points at the lookup
    /// request — this map bridges it back to the payment record.
    pub(super) pending_lookups: HashMap<String, String>,
    /// Sub-id used for the kind:23195 subscription on the NWC relay.
    pub(super) sub_id: String,
    /// Count of kind:23195 responses that arrived with no matching
    /// `pending_payments` entry. Observable via `orphan_response_count()`.
    pub(super) orphan_responses: u64,
    // ── V-79: heartbeat state ──────────────────────────────────────────────
    /// Wall-clock second at which the last heartbeat `get_info` probe was
    /// sent. `0` means no probe has been sent yet in this session.
    pub(super) last_probe_sent_secs: u64,
    /// `true` when a probe was sent and no kind:23195 response has arrived
    /// yet. Reset to `false` by `handle_nwc_text` on any successful response.
    pub(super) probe_outstanding: bool,
    /// Number of consecutive probe windows that elapsed with no response.
    /// Reset to 0 on any successful kind:23195 response.
    pub(super) consecutive_failures: u32,
    /// Coarse transport-health state projected to the snapshot. `None` until
    /// the first probe cadence window has elapsed.
    pub(super) connection_state: Option<NwcConnectionState>,
}

/// Actor-thread-owned NWC runtime. Held behind a [`WalletRuntimeHandle`]
/// slot the actor reaches into per command and per relay message.
pub struct WalletRuntime {
    pub(super) connection: Option<WalletConnection>,
    /// Shared output slot for the wallet projection. The actor (this runtime)
    /// is the sole writer (D4); the `"wallet"` snapshot projection reads it.
    pub(super) status_slot: WalletStatusSlot,
    /// Durable per-payment record store. `None` means in-memory-only (used in
    /// unit tests and pre-startup); `Some` activates the double-pay-safe
    /// write-before-enqueue + tri-state reconciliation path. The host installs
    /// it via [`WalletRuntime::set_payment_store`] using its storage path.
    pub(super) payment_store: Option<FsPaymentStore>,
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
/// carries the handle by value (ADR-0072 rung 5.2). No process-global slot.
#[must_use]
pub fn new_wallet_runtime_handle() -> WalletRuntimeHandle {
    Arc::new(Mutex::new(None))
}

// ADR-0072 rung 5.2: the process-global `ACTIVE_WALLET_RUNTIME`
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
}

mod commands;
mod disconnect;
mod heartbeat;
mod payments;
mod request_builder;

pub use heartbeat::HeartbeatOutbound;
pub use payments::ExpiredPaymentOutcome;

pub(crate) use commands::{handle_nwc_text, wallet_connect, wallet_disconnect, wallet_pay_invoice};

#[path = "../runtime_utils.rs"]
mod runtime_utils;

#[cfg(test)]
#[path = "../runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../runtime_money_path_tests.rs"]
mod money_path_tests;
