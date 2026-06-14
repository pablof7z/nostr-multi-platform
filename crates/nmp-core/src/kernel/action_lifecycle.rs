//! `action_lifecycle` — kernel-owned per-`correlation_id` display projection.
//!
//! # Purpose (thin-shell V5 fix)
//!
//! `action_lifecycle` is the **display projection** the host renders without
//! any reducer-side bookkeeping. It exposes exactly two arrays that drive the
//! host's spinner / toast UI verbatim:
//!
//! * `in_flight` — every correlation_id whose latest recorded stage is
//!   non-terminal (`Requested` / `AwaitingCapability` / `Publishing`). A
//!   host renders a spinner per entry.
//! * `recent_terminal` — every correlation_id that settled (`Accepted` /
//!   `Failed`) within the TTL window. A host renders a success/failure
//!   toast per entry; once the TTL expires the entry drops on its own.
//!
//! # Contrast with `action_stages`
//!
//! `action_stages` is the **full history** of an action's transitions, kept
//! until the host acks. It is the substrate primitive — every stage
//! transition lands there. `action_lifecycle` is a **derived view** over the
//! same transitions, pruned to "what to show on the screen right now":
//!
//! * No ack: terminal entries drop on TTL, not on host signal. The host
//!   never needs to call back into the kernel.
//! * Latest-stage-wins per correlation_id: the history vector collapses to
//!   one `LifecycleStage`.
//! * Bounded retention by wall-clock TTL, not by entry count.
//!
//! Both surfaces are additive — `action_stages` keeps the per-stage detail
//! for diagnostic views, `action_lifecycle` carries the host display shape.
//!
//! # Why this lives in `nmp-core`
//!
//! The data source is [`Kernel::record_action_stage`]; the tracker mirrors
//! every transition. Every app needs lifecycle display (substrate-level
//! concern), and the source data is private to the kernel. Putting the
//! tracker here keeps the projection self-contained — no new public surface
//! on `NmpApp` is required to bridge from app-crate code into the kernel's
//! private state.
//!
//! # D-doctrine
//!
//! * **D6** — TTL drop is silent; a poisoned mutex inside the tracker is
//!   impossible (the tracker is actor-owned, no `Mutex`); serialization
//!   failure of the snapshot value collapses to `Null`.
//! * **D8** — bounded. `MAX_TRACKED_CORRELATIONS` (1024) caps the map size;
//!   drop-oldest by first-record order on overflow. `RECENT_TERMINAL_TTL_MS`
//!   bounds the retention window for settled actions.
//! * **D9** — wall-clock reads route through [`Kernel::now_ms`] so
//!   `FixedClock` makes the TTL sweep deterministic in tests.
//! * **D10 (thin-shell)** — every display string Swift used to compute from
//!   `pendingActions` / `actionStages` is provided here. The shell decodes
//!   `{in_flight, recent_terminal}` verbatim.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::action_stages::ActionStage;

/// Per-tracker map cardinality cap. Mirrors
/// [`super::action_stages::MAX_TRACKED_CORRELATIONS`]: a pathological host
/// that dispatches faster than TTL evicts would otherwise grow this
/// unboundedly. Drop-oldest by first-record order on overflow.
pub(crate) const MAX_TRACKED_CORRELATIONS: usize = 1024;

/// Retention window for terminal entries. After this many milliseconds the
/// `recent_terminal` row drops from the projection on the next snapshot
/// tick. 3 s is the host's UX requirement: long enough for the user to
/// register a success/failure toast, short enough that a quiet failure
/// disappears without manual dismissal.
pub(crate) const RECENT_TERMINAL_TTL_MS: u64 = 3_000;

/// One snapshot of an action's display state. The latest stage observed
/// for a correlation_id collapses to this — no per-transition detail.
///
/// `Failed` carries `reason` as a sibling key (matches
/// [`ActionStage::Failed`]'s `reason` shape so a host parsing both
/// projections sees the same field).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum LifecycleStage {
    Requested,
    AwaitingCapability,
    Publishing,
    Accepted,
    Failed { reason: String },
}

impl LifecycleStage {
    /// `Accepted` / `Failed` count as terminal — they move from
    /// `in_flight` to `recent_terminal`.
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed { .. })
    }

    /// Map the substrate-level [`ActionStage`] into the display-level
    /// [`LifecycleStage`]. The two enums are intentionally distinct: the
    /// substrate type may grow with internal stages the host should not
    /// render verbatim, and the display type may want host-friendly
    /// renamings independent of internal naming.
    fn from_action_stage(stage: ActionStage) -> Self {
        match stage {
            ActionStage::Requested => Self::Requested,
            ActionStage::AwaitingCapability => Self::AwaitingCapability,
            ActionStage::Publishing => Self::Publishing,
            ActionStage::Accepted => Self::Accepted,
            ActionStage::Failed { reason } => Self::Failed { reason },
        }
    }
}

/// One row in either the `in_flight` or `recent_terminal` array. Carries
/// the correlation_id and the latest collapsed stage.
///
/// A `message` field is reserved on `recent_terminal` rows so a future
/// host-facing copy (e.g. `"Note published"`, `"Publish failed: no relays"`)
/// can land without breaking the wire shape. For now `Accepted` emits no
/// message and `Failed` echoes the inner `reason`; both are exposed via
/// `LifecycleStage` so a host can render verbatim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleEntry {
    pub correlation_id: String,
    #[serde(flatten)]
    pub stage: LifecycleStage,
}

/// On-wire snapshot shape — emitted under `projections["action_lifecycle"]`.
///
/// Steady state: both arrays empty → the tracker returns
/// [`serde_json::Value::Null`] from [`ActionLifecycleTracker::snapshot`]
/// so the projection helper omits the key (same convention as
/// `action_results` / `action_stages`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleSnapshot {
    pub in_flight: Vec<LifecycleEntry>,
    pub recent_terminal: Vec<LifecycleEntry>,
}

/// Internal record per tracked correlation_id. Carries the latest stage
/// (collapsed from the per-transition history) plus the wall-clock
/// timestamp of that latest record — used for TTL eviction on terminals
/// and for stable first-record ordering on the global cap eviction.
#[derive(Clone, Debug)]
struct Tracked {
    stage: LifecycleStage,
    /// Wall-clock millis of the *latest* recorded transition. For
    /// non-terminal rows this is purely diagnostic. For terminals it is
    /// the TTL anchor — the row evicts when
    /// `now_ms >= latest_at_ms + RECENT_TERMINAL_TTL_MS`.
    latest_at_ms: u64,
}

/// Actor-owned per-`correlation_id` display projection. Mirrors every
/// transition recorded via [`super::Kernel::record_action_stage`] into a
/// collapsed latest-stage view, then projects into
/// `{ in_flight, recent_terminal }` for the host shell.
#[derive(Default)]
pub(crate) struct ActionLifecycleTracker {
    /// correlation_id → latest collapsed stage + record time.
    entries: HashMap<String, Tracked>,
    /// First-recorded order, parallel to `entries`. Front-popped on global
    /// cap overflow; lazy-pruned on terminal TTL sweep.
    correlation_order: Vec<String>,
    /// D8 visibility: count of evictions caused by the global cardinality
    /// cap. Test-only inspector via the dedicated accessor.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) global_cap_evictions: u64,
}

impl ActionLifecycleTracker {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Mirror a single transition. Called from
    /// [`super::Kernel::record_action_stage`] alongside the substrate
    /// `action_stages` write so both projections see the same edge.
    ///
    /// * A new correlation_id appends to `correlation_order` (back).
    /// * An existing correlation_id keeps its position (drop-oldest is by
    ///   first-record, not last-touch — mirrors `action_stages`).
    /// * On global cap overflow the front of `correlation_order` is
    ///   evicted whole and `global_cap_evictions` increments.
    ///
    /// `at_ms` is the kernel clock at record time. Terminal rows use this
    /// as the TTL anchor; non-terminal rows carry it for diagnostic
    /// inspectors only.
    pub(crate) fn record(&mut self, correlation_id: &str, stage: ActionStage, at_ms: u64) {
        let display_stage = LifecycleStage::from_action_stage(stage);
        let is_new = !self.entries.contains_key(correlation_id);
        if is_new && self.entries.len() >= MAX_TRACKED_CORRELATIONS {
            // Evict the front of the order. Mirrors
            // `ActionStageTracker::record` overflow semantics so the two
            // trackers agree on which correlation_ids survive at cap.
            if let Some(oldest) = self.correlation_order.first().cloned() {
                self.entries.remove(&oldest);
                self.correlation_order.remove(0);
                self.global_cap_evictions = self.global_cap_evictions.saturating_add(1);
            }
        }
        self.entries.insert(
            correlation_id.to_string(),
            Tracked {
                stage: display_stage,
                latest_at_ms: at_ms,
            },
        );
        if is_new {
            self.correlation_order.push(correlation_id.to_string());
        }
    }

    /// Return the current number of tracked entries.
    ///
    /// Used by `action_lifecycle_projection` to detect when `snapshot`'s
    /// `prune_expired` actually removed rows (ADR-0055 Rung 1 codex #3).
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Drop terminal rows whose TTL has expired. Called from
    /// [`Self::snapshot`] so a quiet kernel still prunes on the next
    /// emit. Non-terminal rows are untouched — only the host's progression
    /// of stages can move them, and a host that never settles an action
    /// (deliberate cancel, signer-stalled DM, etc.) will eventually hit
    /// the global cap, which is the safety net.
    fn prune_expired(&mut self, now_ms: u64) {
        let mut drop_ids: Vec<String> = Vec::new();
        for (cid, t) in &self.entries {
            if t.stage.is_terminal()
                && now_ms >= t.latest_at_ms.saturating_add(RECENT_TERMINAL_TTL_MS)
            {
                drop_ids.push(cid.clone());
            }
        }
        if drop_ids.is_empty() {
            return;
        }
        for cid in &drop_ids {
            self.entries.remove(cid);
        }
        self.correlation_order.retain(|cid| !drop_ids.contains(cid));
    }

    /// Project the tracker into the host display shape. Returns
    /// [`serde_json::Value::Null`] when both arrays would be empty so the
    /// snapshot helper omits the projection key in steady state — same
    /// convention as `action_results` / `action_stages`.
    ///
    /// `in_flight` and `recent_terminal` both follow `correlation_order`
    /// so the host renders a stable order across ticks (a freshly
    /// dispatched action lands at the bottom of the list).
    ///
    /// `now_ms` is sourced from the kernel clock so a `FixedClock` makes
    /// the TTL sweep deterministic.
    pub(crate) fn snapshot(&mut self, now_ms: u64) -> serde_json::Value {
        self.prune_expired(now_ms);
        if self.entries.is_empty() {
            return serde_json::Value::Null;
        }
        let mut in_flight: Vec<LifecycleEntry> = Vec::new();
        let mut recent_terminal: Vec<LifecycleEntry> = Vec::new();
        for cid in &self.correlation_order {
            let Some(t) = self.entries.get(cid) else {
                continue;
            };
            let entry = LifecycleEntry {
                correlation_id: cid.clone(),
                stage: t.stage.clone(),
            };
            if t.stage.is_terminal() {
                recent_terminal.push(entry);
            } else {
                in_flight.push(entry);
            }
        }
        // Both arrays empty after prune → return Null so the projection
        // omits its key (steady-state hot path stays free of wasted bytes).
        if in_flight.is_empty() && recent_terminal.is_empty() {
            return serde_json::Value::Null;
        }
        let payload = LifecycleSnapshot {
            in_flight,
            recent_terminal,
        };
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null)
    }

    /// Test/diagnostic accessor: number of tracked correlation_ids
    /// (in_flight + recent_terminal, both pre- and post-TTL — pruning
    /// happens inside [`Self::snapshot`]).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Test-only accessor: does the tracker currently hold an entry for
    /// `correlation_id`? Used by the global-cap-eviction test to assert
    /// the *correct* id was evicted (the oldest by first-record order),
    /// without exposing the private `entries` field.
    #[cfg(test)]
    pub(crate) fn contains(&self, correlation_id: &str) -> bool {
        self.entries.contains_key(correlation_id)
    }
}
