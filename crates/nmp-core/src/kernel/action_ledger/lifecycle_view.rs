//! The `action_lifecycle` display-view types — split out of `action_ledger.rs`
//! to keep that file under the AGENTS.md LOC cap. These are pure data types
//! (plus the small stage-classification/derivation helpers that operate on
//! them); the actual derivation, retention, and observation-gating logic
//! lives in [`super::ActionLedger`].

use serde::{Deserialize, Serialize};

use super::super::action_stages::{ActionStage, PENDING_STAGE_RETENTION_MS};

/// Curated failure reason attached to a `correlation_id` (#1735). The
/// substrate `action_stages` history keeps only the English prose `reason`;
/// the machine-stable `code` (+ optional `subject`) rides the *derived*
/// `action_lifecycle` projection only, so a host can localize it. Stored on the
/// per-`correlation_id` latest-lifecycle slot rather than in the `ActionStage`
/// itself (the substrate stage type stays prose-only — its structured code is
/// S7's, #1754).
#[derive(Clone, Debug, Default)]
pub(super) struct CodedReason {
    pub(super) code: Option<String>,
    pub(super) subject: Option<String>,
}

/// The latest lifecycle state of one `correlation_id` — the authoritative
/// source the `action_lifecycle` view derives from.
///
/// This is a SINGLE slot per id, updated on EVERY recorded transition. It is
/// deliberately INDEPENDENT of the bounded `action_stages` history's
/// per-correlation cap: the history may silently drop a non-terminal diagnostic
/// row once it is full (64 entries), but the latest lifecycle state must still
/// advance — the host's spinner/toast UI keys on it. Co-owned with the history
/// under the one ledger (same key set, same eviction order, reconciled on every
/// mutation) — it is a facet of the single ledger entry, not a parallel store.
#[derive(Clone, Debug)]
pub(super) struct LatestLifecycle {
    /// Latest substrate stage observed for the id (collapsed from the history).
    pub(super) stage: ActionStage,
    /// Wall-clock millis of that latest transition. Pending (non-terminal)
    /// retention is anchored here (unchanged behaviour) — see
    /// [`Self::pending_ttl_expired`]. Terminal retention is anchored to
    /// [`Self::observed_terminal_at_ms`] instead, once set.
    pub(super) at_ms: u64,
    /// Curated failure reason (#1735) attached to the latest transition.
    pub(super) coded: CodedReason,
    /// Wall-clock millis of the FIRST time this entry — while in a terminal
    /// stage — was actually served to a caller of
    /// [`super::ActionLedger::lifecycle_snapshot`] (i.e. included in a
    /// `recent_terminal` row). `None` until that happens.
    ///
    /// This is the chirp#115 fix. A terminal verdict used to be pruned purely
    /// on wall-clock distance from `at_ms` (the transition instant) — racing
    /// whatever was supposed to observe it. A slow emit cadence, a backlogged
    /// host, or simply a relay round-trip that took longer than the terminal
    /// TTL meant the verdict could be evicted before anything ever read it,
    /// wedging a host's spinner forever (it never receives the terminal it is
    /// waiting on). `ActionLedger::prune_latest` now refuses to age out a
    /// terminal entry until it has been observed at least once
    /// (`observed_terminal_at_ms.is_none()` exempts it from TTL pruning
    /// entirely); the terminal retention window then bounds how long it
    /// lingers **after** that first observation, not how long the kernel took
    /// to get around to serving it. A never-observed entry is bounded only by
    /// the ledger's global `MAX_TRACKED_CORRELATIONS` drop-oldest cap — the
    /// same backstop a never-acked entry already relies on, so memory stays
    /// D8-bounded even for a host that never reads at all.
    pub(super) observed_terminal_at_ms: Option<u64>,
}

impl LatestLifecycle {
    /// Whether this NON-TERMINAL slot's pending-retention window has elapsed.
    /// Terminal slots are never checked here — see `ActionLedger::prune_latest`.
    pub(super) fn pending_ttl_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.at_ms.saturating_add(PENDING_STAGE_RETENTION_MS)
    }
}

/// The display-level lifecycle stage. Distinct from the substrate
/// [`ActionStage`]: the substrate type may grow internal stages the host
/// should not render verbatim, and the display type carries the curated
/// `reason_code` / `reason_subject` (#1735) that never bleed into the
/// substrate history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum LifecycleStage {
    Requested,
    AwaitingCapability,
    Publishing,
    Accepted,
    /// `reason` is always the English prose fallback. `reason_code` is the
    /// stable machine key the shell localizes, present ONLY when the kernel set
    /// CURATED app copy (#1735). `reason_subject` is an optional contextual
    /// value the shell interpolates.
    Failed {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason_code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason_subject: Option<String>,
    },
    /// User-initiated cancellation — a DISTINCT terminal from `Failed` (S7,
    /// #1754). The host renders it differently (no error toast).
    Cancelled,
}

impl LifecycleStage {
    /// `Accepted` / `Failed` / `Cancelled` are terminal — they move from
    /// `in_flight` to `recent_terminal`.
    pub(super) fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed { .. } | Self::Cancelled)
    }

    /// Derive a display stage from the latest substrate stage of a
    /// `correlation_id`, attaching the curated reason code (`Failed` only).
    pub(super) fn derive(stage: &ActionStage, coded: &CodedReason) -> Self {
        match stage {
            ActionStage::Requested => Self::Requested,
            ActionStage::AwaitingCapability => Self::AwaitingCapability,
            ActionStage::Publishing => Self::Publishing,
            ActionStage::Accepted => Self::Accepted,
            ActionStage::Failed { reason } => Self::Failed {
                reason: reason.clone(),
                reason_code: coded.code.clone(),
                reason_subject: coded.subject.clone(),
            },
            ActionStage::Cancelled => Self::Cancelled,
        }
    }
}

/// One row in either the `in_flight` or `recent_terminal` array.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleEntry {
    pub correlation_id: String,
    #[serde(flatten)]
    pub stage: LifecycleStage,
}

/// On-wire shape emitted under `projections["action_lifecycle"]`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LifecycleSnapshot {
    pub in_flight: Vec<LifecycleEntry>,
    pub recent_terminal: Vec<LifecycleEntry>,
}
