//! `action_stages` — the [`ActionLedger`]'s stage-history facet substrate.
//!
//! # Why this lives here (S11, #1758 / #1684)
//!
//! This module owns the per-`correlation_id` stage *vocabulary* ([`ActionStage`],
//! [`StageEntry`]) and the bounded transition-history *storage* ([`StageHistory`])
//! that the single [`ActionLedger`] keeps as one of its facets. It is NOT a
//! standalone, separately-maintained tracker: there is exactly one
//! [`StageHistory`] in the system — the ledger's private `stages` field — and the
//! host-facing `action_stages` projection is a *derived view* of it, serialised
//! via [`ActionLedger::stages_snapshot`]. No production path constructs or writes
//! a [`StageHistory`] outside the ledger (S11 slice 3 collapsed the last vestige
//! of a peer tracker into this facet — there is no parallel `action_stages`
//! source of truth).
//!
//! [`ActionLedger`]: super::action_ledger::ActionLedger
//! [`ActionLedger::stages_snapshot`]: super::action_ledger::ActionLedger::stages_snapshot
//!
//! # The shape of the seam
//!
//! `action_results` is a per-tick *drain*: every terminal verdict that
//! settled since the last emit, with the entry dropped after one snapshot.
//! `action_stages` is the bounded diagnostic mirror of an action's lifecycle:
//! the full history of transitions an async action went through (`Requested` →
//! `Publishing` → `Accepted`/`Failed`), retained until host ack or kernel-owned
//! expiry — derived from this facet's storage, not a second copy.
//!
//! The two surfaces are complementary, not redundant:
//!
//! * `action_results` answers "did this action complete?" exactly once per
//!   tick. It is drained on emit because the host's spinner cleanup is a
//!   single edge.
//! * `action_stages` answers "what is this action doing right now?" on every
//!   tick. It is NOT drained on emit because diagnostic consumers may need the
//!   stable state across many ticks. Terminal and pending rows are TTL-retained;
//!   host ack is an early-dismiss path only.
//!
//! # Retention: kernel-owned TTL, ack as early dismissal
//!
//! The kernel owns retention. Terminal rows remain visible until
//! [`TERMINAL_STAGE_RETENTION_MS`] elapses. Pending rows remain visible for
//! [`PENDING_STAGE_RETENTION_MS`], which exceeds the longest signer approval
//! budget. Both drop on the next snapshot emit after expiry. If a host has
//! already reacted, the native-runtime/UniFFI ack method may dismiss the row
//! earlier; correctness does not depend on that host callback.
//!
//! # Caps (D8 — bounded retention)
//!
//! Two dimensions need a cap, both documented and audited:
//!
//! 1. **Per-correlation_id stage history** ([`MAX_STAGES_PER_CORRELATION`]):
//!    every transition appends a [`StageEntry`]. A pathological consumer
//!    that calls `record_action_stage` in a loop would otherwise grow one
//!    entry unboundedly. We cap at 64 — enough for any realistic lifecycle
//!    (Requested + Publishing + N relay-level retries + Accepted/Failed)
//!    while pinning the worst case at ~64 × (key + small JSON detail).
//!
//!    **Terminals are load-bearing — never dropped.** When history reaches
//!    the cap, an incoming `Accepted` / `Failed` evicts the oldest
//!    *non-terminal* entry to make room instead of dropping the terminal.
//!    The host's spinner-cleanup edge (its consumer of `action_results` +
//!    `action_stages`) is keyed on the terminal stage; silently dropping it
//!    under a pathological retry storm would leave the spinner spinning
//!    forever. A non-terminal entry (`Requested` / `Publishing` /
//!    `AwaitingCapability`) is diagnostic — its loss costs a row in the
//!    history view, not a permanently-stuck UI. The terminal *always*
//!    survives; only non-terminals are subject to drop. This makes the cap
//!    an upper bound on *non-terminal* entries (63), not on the whole
//!    history (which can hold the terminal as the 64th).
//!
//! 2. **Map cardinality** ([`MAX_TRACKED_CORRELATIONS`]): a buggy host that
//!    never acks would otherwise accumulate one entry per dispatched
//!    action forever. We cap at 1024 — large enough for any realistic
//!    in-flight backlog, small enough to bound memory at ~1 MiB of stage
//!    JSON. When the cap is exceeded, the *oldest* `correlation_id` (by
//!    insertion order) is evicted whole (drop-oldest semantics, mirroring
//!    [`MAX_CLAIMS_PER_PUBKEY`]) and a counter increments for diagnostic
//!    visibility.
//!
//! Both caps are silent: the new entry is dropped (per-correlation cap) or
//! the oldest correlation is evicted (global cap), and a counter records
//! the event. D6 — a cap hit never panics, never returns an error.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-correlation_id retention cap. A single action's stage history is
/// bounded at 64 entries — well above the realistic Requested →
/// Publishing → Accepted/Failed lifecycle (4) plus any per-relay retries.
pub(crate) const MAX_STAGES_PER_CORRELATION: usize = 64;

/// Global map cardinality cap. A pathological host that never acks any
/// `correlation_id` would otherwise leak one entry per dispatch. We cap at
/// 1024 in-flight tracked actions; the oldest is evicted whole when a new
/// correlation pushes past this.
pub(crate) const MAX_TRACKED_CORRELATIONS: usize = 1024;

/// Terminal retention window shared by the `action_stages` stage-history facet
/// and the display-level `action_lifecycle` projection. After this many milliseconds,
/// a terminal action row drops on the next snapshot/update edge even if no
/// host ack or relay ingest arrives.
pub(crate) const TERMINAL_STAGE_RETENTION_MS: u64 = 3_000;

/// Retention window for pending stage histories. This intentionally exceeds the
/// longest signer approval budget documented in `RemoteSignerHandle::op_timeout`
/// (NIP-55 = 90 s), so projection cleanup does not preempt a legitimate
/// terminal while still bounding a lost `Requested` / `Publishing` /
/// `AwaitingCapability` row.
pub(crate) const PENDING_STAGE_RETENTION_MS: u64 = 120_000;

/// One stage in an async action's lifecycle.
///
/// The vocabulary is intentionally GENERIC (ADR-0071 §lifecycle): signing and
/// publishing are decoupled, neither implies the other, and no stage names a
/// specific capability backend.
///
/// `Requested` fires at dispatch entry (the host called
/// `nmp_app_dispatch_action`; the action was validated and an executor
/// queued). `AwaitingCapability` covers *any* capability round-trip the action
/// blocks on — signing (local key, NIP-07, NIP-46 bunker, NIP-55 Android), MLS,
/// etc. There is deliberately NO signing-specific `WaitingForSignature` stage:
/// signing is one capability among many, invisible to the stage vocabulary
/// (V-78). `Publishing` is a SEPARATE, OPTIONAL stage that fires when the
/// actor's publish engine accepts an event for relay dispatch — it is NOT
/// coupled to signing (an action may sign without publishing, or publish a
/// pre-signed event without an `AwaitingCapability` stage). `Accepted`,
/// `Failed`, and `Cancelled` are the terminals.
///
/// `Cancelled` is a DISTINCT terminal from `Failed`: it marks a USER-initiated
/// cancellation, never a capability/signer denial. A signer or capability
/// rejection is a `Failed { reason }` (carrying a structured `reason_code` on
/// the display projection, #1735), not a `Cancelled`.
///
/// The vocabulary is closed — adding a stage is a schema decision that
/// requires updating the host consumer in lockstep.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ActionStage {
    Requested,
    AwaitingCapability,
    Publishing,
    Accepted,
    Failed {
        reason: String,
    },
    /// User-initiated cancellation — a distinct terminal from `Failed`. Routed
    /// through the cancel-by-`correlation_id` doorway (S7, #1754).
    Cancelled,
}

impl ActionStage {
    /// True for `Accepted` / `Failed` / `Cancelled`. The host typically acks
    /// one tick after observing a terminal stage; non-terminal stages stay in
    /// the snapshot mirror until the eventual ack.
    ///
    // `allow(dead_code)`: iOS `KernelBridge` reads this to gate the auto-ack
    // path; no nmp-core caller exists so the per-crate lint fires spuriously.
    #[allow(dead_code)]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed { .. } | Self::Cancelled)
    }

    fn retention_ttl_ms(&self) -> u64 {
        if self.is_terminal() {
            TERMINAL_STAGE_RETENTION_MS
        } else {
            PENDING_STAGE_RETENTION_MS
        }
    }
}

/// One row in a `correlation_id`'s stage history. Carries the stage, an
/// optional opaque detail payload (relay url, retry count, error text —
/// per-stage convention), and the wall-clock timestamp at which the
/// reducer recorded the transition. `at_ms` is sourced from the kernel
/// clock (`Kernel::now_ms`) so a test `FixedClock` makes the history
/// deterministic.
///
/// The `ActionStage` is flattened so the on-wire shape is a single object:
///
/// ```json
/// {"stage":"publishing","at_ms":123,"detail":{...}}
/// {"stage":"failed","reason":"no relays","at_ms":456}
/// ```
///
/// `Failed`'s `reason` lifts to a sibling of `stage` rather than nesting
/// under an inner object — exactly what a host parsing the snapshot
/// expects when it switches on `stage`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StageEntry {
    #[serde(flatten)]
    pub stage: ActionStage,
    pub at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// The [`ActionLedger`]'s stage-history facet storage — the bounded
/// per-`correlation_id` transition log the `action_stages` projection derives
/// from. There is exactly ONE of these in the system (the ledger's private
/// `stages` field); it is never a standalone, separately-written tracker.
///
/// Insertion order is preserved: `correlation_order` is a parallel ring of
/// keys that grows on first record for a `correlation_id` and shrinks on
/// ack. When the map exceeds [`MAX_TRACKED_CORRELATIONS`] the *front* of
/// the order (oldest first-recorded id) is evicted. The map and the order
/// are kept in sync: every entry in `entries` has exactly one matching
/// slot in `correlation_order`.
///
/// [`ActionLedger`]: super::action_ledger::ActionLedger
#[derive(Default)]
pub(crate) struct StageHistory {
    /// `correlation_id` → ordered stage history.
    entries: HashMap<String, Vec<StageEntry>>,
    /// First-recorded order of `correlation_ids`; the oldest entry is
    /// evicted when the map exceeds [`MAX_TRACKED_CORRELATIONS`].
    correlation_order: Vec<String>,
    /// D8 visibility: count of evictions caused by the global cardinality
    /// cap. Exposed to tests; production diagnostics can fold this in
    /// later via a snapshot metric if needed.
    pub(crate) global_cap_evictions: u64,
    /// D8 visibility: count of stage appends rejected by the
    /// per-correlation cap. Exposed to tests.
    ///
    /// Only ever incremented for *non-terminal* stages at cap. A terminal
    /// stage at cap evicts the oldest non-terminal entry instead of
    /// dropping itself — see [`Self::record`] — and bumps
    /// `per_correlation_terminal_evictions` rather than this counter.
    pub(crate) per_correlation_cap_drops: u64,
    /// D8 visibility: count of non-terminal entries evicted to make room
    /// for an incoming terminal stage when the per-correlation history
    /// hits [`MAX_STAGES_PER_CORRELATION`]. Distinct from
    /// `per_correlation_cap_drops` so a test can prove the terminal
    /// survival contract (the terminal arrived, the diagnostic was lost).
    pub(crate) per_correlation_terminal_evictions: u64,
}

impl StageHistory {
    /// The [`super::action_ledger::ActionLedger`] owns this facet via
    /// `Default`; this explicit constructor exists ONLY for the white-box
    /// stage-history unit tests, which exercise the cap/eviction/TTL substrate
    /// directly. Production code never constructs a [`StageHistory`] — it lives
    /// solely inside the one ledger.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Append `stage` (with optional `detail`) onto `correlation_id`'s
    /// history, stamped at `at_ms`. New `correlation_ids` are placed at the
    /// back of the eviction order; existing ids retain their original
    /// position so a long-running action does not get re-prioritised by
    /// activity (drop-oldest is by first-record, not by last-touch — the
    /// `MAX_CLAIMS_PER_PUBKEY` convention).
    ///
    /// Cap behaviour:
    /// * If the per-correlation history is at
    ///   [`MAX_STAGES_PER_CORRELATION`] and the incoming stage is a
    ///   **terminal** (`Accepted` / `Failed`), the oldest *non-terminal*
    ///   entry in the history is evicted to make room, and
    ///   `per_correlation_terminal_evictions` increments. The terminal
    ///   always survives — the host's spinner-cleanup edge depends on it.
    ///   If the history somehow contains only terminals (e.g. a buggy
    ///   producer recording 64 `Accepted` rows on the same id) the
    ///   incoming terminal IS the canonical one, so the oldest terminal
    ///   is evicted; the contract "the latest terminal survives" still
    ///   holds.
    /// * If the per-correlation history is at the cap and the incoming
    ///   stage is **non-terminal**, the call is a silent no-op and
    ///   `per_correlation_cap_drops` increments — the diagnostic loss is
    ///   safe (a non-terminal stage never drives UI cleanup).
    /// * If the global map would exceed [`MAX_TRACKED_CORRELATIONS`] the
    ///   oldest `correlation_id` (front of `correlation_order`) is evicted
    ///   wholesale, `global_cap_evictions` increments, and the append
    ///   proceeds.
    pub(crate) fn record(
        &mut self,
        correlation_id: &str,
        stage: ActionStage,
        detail: Option<serde_json::Value>,
        at_ms: u64,
    ) {
        let is_new = !self.entries.contains_key(correlation_id);
        if is_new && self.entries.len() >= MAX_TRACKED_CORRELATIONS {
            // Evict the front of the order. If the order somehow desyncs
            // from the map (an invariant break), this still terminates —
            // a missing key is a no-op and the loop will eventually pop
            // a real entry or empty the order.
            if let Some(oldest) = self.correlation_order.first().cloned() {
                self.entries.remove(&oldest);
                self.correlation_order.remove(0);
                self.global_cap_evictions = self.global_cap_evictions.saturating_add(1);
            }
        }
        let stage_is_terminal = stage.is_terminal();
        let history = self.entries.entry(correlation_id.to_string()).or_default();
        if history.len() >= MAX_STAGES_PER_CORRELATION {
            if stage_is_terminal {
                // Terminals MUST survive: evict the oldest non-terminal entry
                // (preserving prior terminals — a buggy producer recording a
                // chain of terminals stays observable). Fallback: if the
                // history is solely terminals (degenerate), evict the oldest
                // one — the latest terminal is the canonical one and still
                // survives.
                let evict_idx = history
                    .iter()
                    .position(|e| !e.stage.is_terminal())
                    .unwrap_or(0);
                history.remove(evict_idx);
                self.per_correlation_terminal_evictions =
                    self.per_correlation_terminal_evictions.saturating_add(1);
                // Fall through to push the terminal below.
            } else {
                // Non-terminal at cap: silent no-op. Diagnostic loss is safe.
                self.per_correlation_cap_drops = self.per_correlation_cap_drops.saturating_add(1);
                return;
            }
        }
        history.push(StageEntry {
            stage,
            at_ms,
            detail,
        });
        if is_new {
            self.correlation_order.push(correlation_id.to_string());
        }
    }

    /// Drop the entry for `correlation_id`. Idempotent: an unknown id is a
    /// silent no-op (D6 — a bad ack never crashes). Returns `true` when an
    /// entry was actually removed, exposed for the test that asserts the
    /// host's ack-of-unknown is a no-op rather than a side-effect.
    pub(crate) fn ack(&mut self, correlation_id: &str) -> bool {
        let removed = self.entries.remove(correlation_id).is_some();
        if removed {
            // Order vector follows the map; O(N) pop here is fine — the
            // global cap pins N ≤ MAX_TRACKED_CORRELATIONS.
            if let Some(pos) = self
                .correlation_order
                .iter()
                .position(|id| id == correlation_id)
            {
                self.correlation_order.remove(pos);
            }
        }
        removed
    }

    /// Drop entries whose retention window has elapsed.
    ///
    /// The latest stage is the authoritative lifecycle state. Terminal rows use
    /// the short display TTL; pending rows use the longer pending-action TTL.
    /// Both expire from the kernel-owned snapshot edge without waiting for a
    /// host ack.
    pub(crate) fn prune_expired(&mut self, now_ms: u64) -> usize {
        let mut drop_ids: Vec<String> = Vec::new();
        for (cid, history) in &self.entries {
            let Some(latest) = history.last() else {
                drop_ids.push(cid.clone());
                continue;
            };
            if now_ms >= latest.at_ms.saturating_add(latest.stage.retention_ttl_ms()) {
                drop_ids.push(cid.clone());
            }
        }
        if drop_ids.is_empty() {
            return 0;
        }
        for cid in &drop_ids {
            self.entries.remove(cid);
        }
        self.correlation_order.retain(|cid| !drop_ids.contains(cid));
        drop_ids.len()
    }

    /// Current number of tracked correlation ids.
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Serialize every tracked `correlation_id`'s history into the JSON
    /// shape the snapshot mirror exposes:
    /// `{ "<correlation_id>": [ { "stage": ..., "at_ms": ..., ... }, ... ], ... }`.
    ///
    /// Returns `serde_json::Value::Null` when nothing is tracked, so the
    /// projection helper (`update.rs`) can omit the key in steady state
    /// — exactly the convention `action_results` uses for "no rows this
    /// tick". Expired entries are pruned first, using the caller-supplied kernel
    /// clock timestamp. This is otherwise a *copy* (clone semantics, not move);
    /// the map is not drained by serialization.
    pub(crate) fn snapshot(&mut self, now_ms: u64) -> serde_json::Value {
        self.prune_expired(now_ms);
        if self.entries.is_empty() {
            return serde_json::Value::Null;
        }
        let map: serde_json::Map<String, serde_json::Value> = self
            .entries
            .iter()
            .map(|(cid, history)| {
                let arr: Vec<serde_json::Value> = history
                    .iter()
                    .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
                    .collect();
                (cid.clone(), serde_json::Value::Array(arr))
            })
            .collect();
        serde_json::Value::Object(map)
    }

    /// Test/diagnostic accessor: snapshot of the order vector so the cap
    /// eviction test can assert the front-pop semantics without poking
    /// private fields. Cheap (clone of `Vec<String>`) but kept behind
    /// `#[cfg(test)]` so it does not appear in production callsites.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn order_snapshot(&self) -> Vec<String> {
        self.correlation_order.clone()
    }

    /// Test/diagnostic accessor: number of tracked correlation_ids.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Test/diagnostic accessor: stage history for a correlation_id.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn history(&self, correlation_id: &str) -> Option<&[StageEntry]> {
        self.entries.get(correlation_id).map(|v| v.as_slice())
    }
}

#[cfg(test)]
#[path = "action_stages/tests.rs"]
mod tests;
