//! `WireSub` — active wire (WebSocket) subscription bookkeeping row.
//!
//! Split out of `kernel/types.rs` (LOC cap) as a cohesive owner. One row per
//! `(relay_url, sub_id)` in `WireSubscriptionState::subs`; the EOSE / CLOSED
//! ingest handlers and the relay-diagnostics projection read it.

use super::{CanonicalRelayUrl, Instant, RelayRole};

/// Active wire (WebSocket) subscription state.
///
/// T105: `relay_url` is the resolved wire target this sub was opened on. The
/// CLOSE frame for this sub-id must be routed back to the same `relay_url`
/// (the transport pool is URL-keyed, so closing on the wrong socket would
/// leave the original subscription open). `role` is the transport lane label.
pub(crate) struct WireSub {
    pub(super) id: String,
    pub(super) role: RelayRole,
    /// Resolved relay URL this subscription was opened on (T105). The CLOSE
    /// frame for `id` must target this URL — the transport pool is URL-keyed
    /// and would otherwise leak the open subscription on the original relay.
    /// Canonical by construction: this field mirrors the `wire_subs` key half.
    pub(super) relay_url: CanonicalRelayUrl,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) events_rx: u64,
    pub(super) opened_at: Instant,
    pub(super) last_event_at: Option<Instant>,
    pub(super) eose_at: Option<Instant>,
    pub(super) close_reason: Option<String>,
    /// K3 Stage D1 (ADR-0056 §3) — the `since` floor (unix-seconds) on the REQ
    /// filter this sub was opened with, or `None` if the REQ was un-floored. The
    /// EOSE handler reads it to record coverage honestly: an un-floored REQ
    /// (`None`/`Some(0)`) proves `[0, now]` and advances the ledger; a floored
    /// REQ proves only `[floor, now]`, so its EOSE records nothing rather than
    /// over-claim `[0, floor)` (see `Kernel::record_eose_coverage`).
    pub(super) since_floor: Option<u64>,
}
