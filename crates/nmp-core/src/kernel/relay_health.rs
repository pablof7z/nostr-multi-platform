//! Per-relay transport health, diagnostics counters, and wire-subscription
//! registry state — plus the host projections derived from them.
//!
//! Owns the runtime health/state (`RelayHealth`, `Counters`, `NoticeEntry`,
//! `WireSubscriptionState`) and the snapshot projections that surface it to
//! shells (`RelayStatus`, `WireSubscriptionStatus`, `LogicalInterestStatus`).
//! `status.rs` projects `RelayHealth` → `RelayStatus`.

use std::collections::VecDeque;

use super::wire_sub::WireSub;
use super::{CanonicalRelayUrl, HashMap, HashSet, Instant, Serialize};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RelayStatus {
    pub(super) role: String,
    pub(super) relay_url: String,
    pub(super) connection: String,
    pub(super) auth: String,
    pub(super) negentropy_probe: String,
    pub(super) active_wire_subscriptions: usize,
    pub(super) reconnect_count: u32,
    pub(super) last_connected_at_ms: Option<u128>,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) last_notice: Option<String>,
    pub(super) last_error: Option<String>,
    /// Machine-readable category for `last_error`. Closed key set:
    /// `auth_required | transient | permanent | malformed_event | policy_denied`.
    /// `None` when `last_error` is empty. Lets iOS branch on error *class*
    /// without substring-matching the English `last_error` prose.
    pub(super) error_category: Option<String>,
    pub(super) events_rx: u64,
    /// Total NOTICE frames received on this relay (sourced from
    /// `Counters::notices_rx`). Projected into `RelayDiagnosticsRow.notice_count`.
    pub(super) notices_rx: u64,
    /// Bounded NOTICE log (newest first in the projection; here the Vec carries
    /// them in arrival order for cheap map). Sourced from `RelayHealth.notices`
    /// / `RelayTransportStatus.notices`.
    /// Excluded from serde: carries through typed-projection path only.
    #[serde(skip)]
    pub(super) notices: Vec<NoticeEntry>,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
    /// T120 (G8 / G11): relay has denied this client by policy
    /// (NIP-01 CLOSED reason `restricted:`, `blocked:`, or `shadowbanned:`).
    /// Set once a denial classification arrives; surfaces in diagnostics so
    /// UIs and reconnect workers can suppress retries against this relay.
    pub(super) denied: bool,
    /// T120 (G8 / G11): diagnostic key for the most recent NIP-01 CLOSED
    /// reason prefix (`auth-required`, `rate-limited`, `restricted`, …) —
    /// matches `CloseReason::as_key()`. `None` until the first classified
    /// CLOSED frame arrives.
    pub(super) last_close_reason: Option<String>,
    /// ADR-0051 — the relay's NIP-11 information document, once `nmp-nip11`
    /// has fetched it for this URL. `None` until the fetch resolves (or if
    /// the relay serves no document). The carried-through `RelayInfoDoc` is
    /// substrate-generic transport metadata (D0).
    ///
    /// Surfaced through `relay_diagnostics` as both the serde-JSON subtree and
    /// the `KRDG` typed FlatBuffers sidecar (`InfoRow`).
    pub(super) info: Option<crate::substrate::RelayInfoDoc>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WireSubscriptionStatus {
    pub(super) wire_id: String,
    pub(super) relay_url: String,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) logical_consumer_count: u32,
    pub(super) events_rx: u64,
    pub(super) opened_at_ms: u128,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) eose_at_ms: Option<u128>,
    pub(super) close_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LogicalInterestStatus {
    pub(super) key: String,
    pub(super) state: String,
    pub(super) refcount: u32,
    pub(super) relay_urls: Vec<String>,
    pub(super) cache_coverage: String,
    pub(super) warming_until_ms: Option<u128>,
}

/// One entry in the per-relay bounded NOTICE log.
///
/// Populated at the SAME capture hook that sets `RelayHealth.last_notice` /
/// `RelayTransportStatus.last_notice`, with a wall-clock Unix-ms timestamp
/// (`self.now_ms()`) so the log is independently renderable without a
/// started-at anchor. The ring is capped at [`MAX_NOTICE_LOG`] entries
/// (oldest-dropped) to bound memory.
#[derive(Clone, Debug, Serialize)]
pub(super) struct NoticeEntry {
    /// Wall-clock Unix epoch milliseconds when this NOTICE arrived.
    pub(super) at_ms: u64,
    /// Notice text (truncated to 180 chars at the capture site).
    pub(super) text: String,
}

/// Maximum number of NOTICE entries retained per relay in the bounded log.
pub(super) const MAX_NOTICE_LOG: usize = 32;

/// Per-relay rolling counters for diagnostics.
#[derive(Clone, Debug, Default)]
pub(super) struct Counters {
    pub(super) frames_rx: u64,
    pub(super) events_rx: u64,
    pub(super) eose_rx: u64,
    pub(super) notices_rx: u64,
    pub(super) closed_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
}

/// Per-relay health state: connection status, timestamps, and counters.
#[derive(Clone, Debug)]
pub(super) struct RelayHealth {
    pub(super) connection: String,
    pub(super) connected_at: Option<Instant>,
    pub(super) last_event_at: Option<Instant>,
    pub(super) last_notice: Option<String>,
    /// Bounded NOTICE log (oldest first, capped at [`MAX_NOTICE_LOG`]). Each
    /// entry carries a wall-clock Unix-ms timestamp from `now_ms()`. Populated
    /// alongside `last_notice` in the NOTICE capture hook; projected into
    /// `RelayDiagnosticsRow.notices` + `RelayDiagnosticsRow.notice_count`.
    pub(super) notices: VecDeque<NoticeEntry>,
    pub(super) last_error: Option<String>,
    /// Machine-readable category for `last_error`. Closed key set:
    /// `auth_required | transient | permanent | malformed_event | policy_denied`
    /// (see [`crate::kernel::closed_reason`] for the constants). Stamped
    /// alongside `last_error` and cleared with it. Projected into
    /// `RelayStatus::error_category` by `status.rs`.
    pub(super) error_category: Option<String>,
    pub(super) reconnect_count: u32,
    pub(super) counters: Counters,
    /// NIP-42 per-relay auth state — diagnostic key matching ADR-0007 wire
    /// keys (`not_required` | `challenge_received` | `authenticating` |
    /// `authenticated` | `failed`). Mutated by `handle_auth_challenge` /
    /// `handle_auth_ok` per D8 (without bumping `changed_since_emit`).
    pub(super) auth: String,
    /// T120 (G8 / G11): set when the relay has denied this client by policy
    /// (NIP-01 CLOSED `restricted:` / `blocked:` / `shadowbanned:`). The
    /// reconnect/REQ machinery should treat a denied relay as offline-for-
    /// this-client; recovery is a fresh socket only (relay edit, etc.).
    pub(super) denied: bool,
    /// T120 (G8 / G11): the diagnostic key of the most recently classified
    /// NIP-01 CLOSED reason. `None` until the first classified frame arrives.
    pub(super) last_close_reason: Option<String>,
    /// T112 — negentropy probe state for this relay, as a diagnostic
    /// string key (`"unknown"` | `"probing"` | `"supported"` | `"unsupported"`).
    /// Negentropy is a generic relay-side reconciliation capability; its
    /// concrete NIP binding lives in a downstream protocol crate, so this
    /// substrate field stays NIP-agnostic. Stored as a plain string so
    /// `nmp-core` does not depend on any shell-side probe-state type (D0 —
    /// no cycle). Updated by the actor/observer layer via
    /// `Kernel::set_negentropy_probe_state` whenever the capability probe
    /// transitions; see `status.rs` for the projection into
    /// `RelayStatus::negentropy_probe`.
    pub(super) negentropy_probe_state: String,
}

impl Default for RelayHealth {
    fn default() -> Self {
        Self {
            connection: "offline".to_string(),
            connected_at: None,
            last_event_at: None,
            last_notice: None,
            notices: VecDeque::new(),
            last_error: None,
            error_category: None,
            reconnect_count: 0,
            counters: Counters::default(),
            auth: "not_required".to_string(),
            denied: false,
            last_close_reason: None,
            negentropy_probe_state: "unknown".to_string(),
        }
    }
}

/// Wire (WebSocket) subscription bookkeeping. `subs` is the per-`(relay_url,
/// sub_id)` registry; `persistent` is the set of `(relay_url, sub_id)` pairs
/// that must survive EOSE (NWC-style long-lived listeners). Grouped because the
/// EOSE/CLOSED handlers in `ingest/mod.rs` and the REQ paths in `requests/`
/// touch both in lockstep — see the `wire_subs` field doc on `Kernel` for the
/// #170 relay-scoped-keying rationale.
#[derive(Default)]
pub(super) struct WireSubscriptionState {
    /// Wire-sub bookkeeping keyed by `(relay_url, sub_id)`.
    pub(super) subs: HashMap<(CanonicalRelayUrl, String), WireSub>,
    /// `(relay_url, sub_id)` pairs pinned open across EOSE.
    pub(super) persistent: HashSet<(CanonicalRelayUrl, String)>,
}
