//! `action_stages` — actor-owned per-`correlation_id` lifecycle tracking.
//!
//! # The shape of the seam
//!
//! `action_results` is a per-tick *drain*: every terminal verdict that
//! settled since the last emit, with the entry dropped after one snapshot.
//! `action_stages` is the *mirror* of an action's lifecycle WHILE it is
//! in flight — the full history of non-terminal transitions an async action
//! went through (`Requested` → `Publishing` → `Accepted`/`Failed`), kept on
//! the snapshot until the host acknowledges it.
//!
//! The two surfaces are complementary, not redundant:
//!
//! * `action_results` answers "did this action complete?" exactly once per
//!   tick. It is drained on emit because the host's spinner cleanup is a
//!   single edge.
//! * `action_stages` answers "what is this action doing right now?" on every
//!   tick. It is NOT drained on emit because the host's progress indicator
//!   needs the stable state across many ticks; it persists until the host
//!   *acks* the `correlation_id` via `nmp_app_ack_action_stage`.
//!
//! # Retention: ack-based (option B)
//!
//! The host owns the entry's lifetime. After the host has reacted to the
//! terminal stage (`Accepted` or `Failed`) and freed its UI state, it calls
//! `nmp_app_ack_action_stage(correlation_id)`. The kernel drops the entry
//! from `entries` at that moment, NOT on a TTL and NOT on terminal-stage
//! emission. This is the only race-free option: any TTL or implicit drop
//! risks a host that hasn't yet processed the terminal stage losing it on
//! the next tick.
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

/// One stage in an async action's lifecycle.
///
/// `Requested` fires at dispatch entry (the host called
/// `nmp_app_dispatch_action`; the action was validated and an executor
/// queued). `AwaitingCapability` is reserved for actions that block on a
/// host-side capability (NIP-46 remote sign, MLS, etc.) — emitted only by
/// modules that actually wait. `Publishing` fires when the actor's publish
/// engine accepts the event for relay dispatch. `Accepted` / `Failed` are
/// the terminals.
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
    Failed { reason: String },
}

impl ActionStage {
    /// True for `Accepted` / `Failed`. The host typically acks one tick
    /// after observing a terminal stage; non-terminal stages stay in the
    /// snapshot mirror until the eventual ack.
    ///
    /// `allow(dead_code)`: used by callers outside the crate (the iOS
    /// shell's `KernelBridge` decodes the stage and reads this to gate the
    /// auto-ack path); no internal `nmp-core` site consumes it, so rustc's
    /// per-crate dead-code lint cannot see the live usage.
    #[allow(dead_code)]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed { .. })
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

/// Actor-owned per-correlation_id stage tracker.
///
/// Insertion order is preserved: `correlation_order` is a parallel ring of
/// keys that grows on first record for a `correlation_id` and shrinks on
/// ack. When the map exceeds [`MAX_TRACKED_CORRELATIONS`] the *front* of
/// the order (oldest first-recorded id) is evicted. The map and the order
/// are kept in sync: every entry in `entries` has exactly one matching
/// slot in `correlation_order`.
#[derive(Default)]
pub(crate) struct ActionStageTracker {
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

impl ActionStageTracker {
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

    /// Serialize every tracked `correlation_id`'s history into the JSON
    /// shape the snapshot mirror exposes:
    /// `{ "<correlation_id>": [ { "stage": ..., "at_ms": ..., ... }, ... ], ... }`.
    ///
    /// Returns `serde_json::Value::Null` when nothing is tracked, so the
    /// projection helper (`update.rs`) can omit the key in steady state
    /// — exactly the convention `action_results` uses for "no rows this
    /// tick". This is a *copy* (clone semantics, not move); the internal
    /// map is unchanged by serialization — that is the point of the
    /// mirror vs. drain split.
    pub(crate) fn snapshot(&self) -> serde_json::Value {
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
