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

/// NIP-47 wallet connection status — projected to the snapshot under
/// `projections["wallet"]`.
///
/// RAW-DATA DOCTRINE (aim.md §2 / ADR-0032): every field here is a raw
/// semantic token. No pre-rendered English label, tone string, or
/// display-formatted number lives in this projection — the shells render the
/// status label, semantic tone (colour/icon), and thousands-separated balance
/// themselves from the raw `status` token + `balance_sats`. The earlier
/// `status_label` / `status_tone` / `balance_sats_display` precompute was a
/// presentation regression (#623) removed in the wallet_status sweep (analogous
/// to the #1580 signer-state sweep). `wallet_npub_short` was a further
/// presentation regression (#1678, D7) removed similarly — shells abbreviate
/// `wallet_npub` themselves.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WalletStatus {
    /// Raw NIP-47 status token the shells map to a label/tone themselves:
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`.
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
    /// responds to `get_balance`. The shell applies its own thousands-separator
    /// formatting when rendering (raw-data doctrine).
    pub balance_sats: Option<u64>,
    /// `status == "ready"`. A raw boolean predicate over the status token (not a
    /// presentation string) — pre-computed so the shell can bind a `Bool`
    /// without re-deriving from the status string (thin-shell V-23).
    pub is_ready: bool,
    /// `status == "connecting" || status == "ready"`. A raw boolean predicate
    /// over the status token. Pre-computed for the shell (thin-shell V-23).
    pub is_connected: bool,
    /// V-79: real-time transport-health state derived from the periodic
    /// heartbeat probe. `None` until the first heartbeat interval elapses
    /// (i.e. for the first ~30 s after connect, when we have no probe data
    /// yet). The shell renders a non-silent indicator when this is
    /// `Reconnecting` or `TransportLost`.
    pub connection_state: Option<NwcConnectionState>,
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
    fn new_wallet_status_slot_is_empty() {
        let slot = new_wallet_status_slot();
        assert!(slot.lock().unwrap().is_none());
    }
}
