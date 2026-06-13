//! NIP-47 wallet connection status — the app noun projected onto the snapshot
//! under `projections["wallet"]`.
//!
//! Moved from `nmp-core::actor::commands::wallet` (V-38). The kernel never
//! names this type; a host-registered snapshot projection reads the shared
//! [`WalletStatusSlot`] on every tick (D0 — the kernel emits, never names a
//! host noun).

use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Coarse-grained NWC transport-health state projected alongside [`WalletStatus`].
///
/// V-79: the host shell binds this to render a non-silent liveness indicator
/// even when `status == "ready"` (which reflects the last *protocol* state, not
/// real-time relay reachability).
///
/// Transitions:
/// * `Connected` — at least one successful heartbeat response was received
///   within the probe window; the connection is believed healthy.
/// * `Reconnecting` — ≥ `HEARTBEAT_MAX_FAILURES` consecutive probes went
///   unanswered; a re-subscription was issued and we are waiting for a fresh
///   get_info response.
/// * `TransportLost` — ≥ `HEARTBEAT_MAX_FAILURES` consecutive probes went
///   unanswered *after* a resubscribe was already attempted; the relay itself
///   appears unreachable. The user must manually reconnect.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NwcConnectionState {
    /// Transport believed healthy; last heartbeat probe was answered.
    Connected,
    /// Probes missed; a resubscribe was issued; awaiting confirmation.
    Reconnecting,
    /// Resubscribe also unanswered — relay is considered unreachable.
    TransportLost,
}

/// Semantic tone vocabulary for pre-computed display hints (ADR-0032 / #623).
///
/// The shell maps tone → colour/icon without any protocol-string knowledge.
/// This vocabulary is reusable for other projections that follow the same
/// pattern (e.g. `bunker_connection_state` label/tone — issue #1099).
///
/// String constants rather than an enum so the wire stays `String` and
/// forward-compat is trivially additive (unknown tone → treat as `inactive`).
pub mod tone {
    /// Healthy / normal operating state — green indicator.
    pub const ACTIVE: &str = "active";
    /// Transient degraded state (connecting, reconnecting) — amber indicator.
    pub const WARNING: &str = "warning";
    /// Terminal error state — red indicator.
    pub const ERROR: &str = "error";
    /// Not connected / no session — neutral/secondary indicator.
    pub const INACTIVE: &str = "inactive";
}

/// Derive the human-readable label for a raw NIP-47 wire status token.
///
/// `"connecting"` → `"Connecting"`, `"ready"` → `"Ready"`,
/// `"error"` → `"Error"`, `"disconnected"` → `"Disconnected"`,
/// anything else → `"Unknown"`.
#[must_use]
pub fn status_label(wire_status: &str) -> String {
    match wire_status {
        "connecting" => "Connecting".to_string(),
        "ready" => "Ready".to_string(),
        "error" => "Error".to_string(),
        "disconnected" => "Disconnected".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// Derive the semantic tone for a raw NIP-47 wire status token.
///
/// Maps to the vocabulary in [`tone`]:
/// * `"ready"` → `"active"` (green)
/// * `"connecting"` → `"warning"` (amber — transient)
/// * `"error"` → `"error"` (red)
/// * `"disconnected"` / anything else → `"inactive"` (neutral)
#[must_use]
pub fn status_tone(wire_status: &str) -> String {
    match wire_status {
        "ready" => tone::ACTIVE.to_string(),
        "connecting" => tone::WARNING.to_string(),
        "error" => tone::ERROR.to_string(),
        _ => tone::INACTIVE.to_string(),
    }
}

/// NIP-47 wallet connection status — projected to the snapshot under
/// `projections["wallet"]`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WalletStatus {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    pub status: String,
    /// The NWC relay URL (from the connection URI).
    pub relay_url: String,
    /// The wallet service pubkey in bech32 npub form.
    pub wallet_npub: String,
    /// The wallet service pubkey in raw hex form (64 chars). The shell formats
    /// it for display (bech32 / abbreviation are presentation concerns —
    /// ADR-0032). Sourced from the same NWC connection the `wallet_npub` is
    /// derived from (`WalletConnection.wallet_pubkey_hex`).
    pub wallet_pubkey_hex: String,
    /// Balance in millisatoshis, if the wallet has responded to `get_balance`.
    pub balance_msats: Option<u64>,
    /// Satoshi balance (= `balance_msats / 1000`). `None` until the wallet
    /// responds to `get_balance`.
    pub balance_sats: Option<u64>,
    /// Human-readable balance with thousands separators (`"12,345"`). `None`
    /// when `balance_sats` is `None`. Lets the shell bind without Swift
    /// `formatted()` (thin-shell V-23).
    pub balance_sats_display: Option<String>,
    /// Abbreviated npub: first 10 chars + `"…"` + last 6 chars. Replaces
    /// the Swift `shortNpub()` helper (thin-shell V-23).
    pub wallet_npub_short: String,
    /// `status == "ready"`. Pre-computed so the shell can bind a `Bool`
    /// without re-deriving from the status string (thin-shell V-23).
    pub is_ready: bool,
    /// `status == "connecting" || status == "ready"`. Pre-computed for the
    /// shell (thin-shell V-23).
    pub is_connected: bool,
    /// V-79: real-time transport-health state derived from the periodic
    /// heartbeat probe. `None` until the first heartbeat interval elapses
    /// (i.e. for the first ~30 s after connect, when we have no probe data
    /// yet). The shell renders a non-silent indicator when this is
    /// `Reconnecting` or `TransportLost`.
    pub connection_state: Option<NwcConnectionState>,
    /// ADR-0032 / #623: pre-computed human-readable label for the current
    /// status (e.g. `"Connecting"`, `"Ready"`, `"Error"`, `"Disconnected"`).
    /// The shell binds this verbatim — no Swift/Kotlin string manipulation
    /// required (thin-shell rule). Derived by [`status_label`].
    pub status_label: String,
    /// ADR-0032 / #623: semantic tone for the current status, one of
    /// `"active"` | `"warning"` | `"error"` | `"inactive"` (see [`tone`]).
    /// The shell maps tone → colour/icon without knowing protocol strings.
    /// Derived by [`status_tone`].
    pub status_tone: String,
}

/// Format a satoshi count with `,` thousands separators (e.g. `12345` →
/// `"12,345"`). Replaces the Swift `sats.formatted()` call site
/// (thin-shell V-23).
#[must_use]
pub fn format_sats_display(sats: u64) -> String {
    let s = sats.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Shared wallet-status slot — the output side of the wallet projection.
///
/// One `Arc` clone lives on the actor's [`WalletRuntime`](crate::runtime::WalletRuntime)
/// (the sole writer, D4); another is captured by the `"wallet"` snapshot-
/// projection closure registered on `NmpApp`. The projection reads this slot
/// on every snapshot tick and serializes its contents into
/// `KernelSnapshot::projections`.
///
/// `None` (the default) means no wallet has been connected this session — the
/// projection then contributes JSON `null` under the `"wallet"` key,
/// preserving the "key present, value null when disconnected" semantic the
/// social shells already decode.
pub type WalletStatusSlot = Arc<Mutex<Option<WalletStatus>>>;

/// Construct a fresh, empty [`WalletStatusSlot`].
#[must_use]
pub fn new_wallet_status_slot() -> WalletStatusSlot {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_sats_display_inserts_thousands_separators() {
        assert_eq!(format_sats_display(0), "0");
        assert_eq!(format_sats_display(12), "12");
        assert_eq!(format_sats_display(1_234), "1,234");
        assert_eq!(format_sats_display(12_345), "12,345");
        assert_eq!(format_sats_display(123_456_789), "123,456,789");
    }

    #[test]
    fn new_wallet_status_slot_is_empty() {
        let slot = new_wallet_status_slot();
        assert!(slot.lock().unwrap().is_none());
    }

    // ADR-0032 / #623: label and tone for each wire status token.

    #[test]
    fn status_label_connecting() {
        assert_eq!(status_label("connecting"), "Connecting");
    }

    #[test]
    fn status_label_ready() {
        assert_eq!(status_label("ready"), "Ready");
    }

    #[test]
    fn status_label_error() {
        assert_eq!(status_label("error"), "Error");
    }

    #[test]
    fn status_label_disconnected() {
        assert_eq!(status_label("disconnected"), "Disconnected");
    }

    #[test]
    fn status_label_unknown_token_falls_back() {
        assert_eq!(status_label("some_future_state"), "Unknown");
    }

    #[test]
    fn status_tone_ready_is_active() {
        assert_eq!(status_tone("ready"), tone::ACTIVE);
    }

    #[test]
    fn status_tone_connecting_is_warning() {
        assert_eq!(status_tone("connecting"), tone::WARNING);
    }

    #[test]
    fn status_tone_error_is_error() {
        assert_eq!(status_tone("error"), tone::ERROR);
    }

    #[test]
    fn status_tone_disconnected_is_inactive() {
        assert_eq!(status_tone("disconnected"), tone::INACTIVE);
    }

    #[test]
    fn status_tone_unknown_is_inactive() {
        assert_eq!(status_tone("some_future_state"), tone::INACTIVE);
    }
}
