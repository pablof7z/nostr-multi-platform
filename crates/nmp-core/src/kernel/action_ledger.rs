//! `ActionLedger` — the single per-`correlation_id` record of action state.
//!
//! # Why this exists (S11, #1758 / #1684)
//!
//! Action outcomes used to be recorded into FOUR overlapping surfaces, each
//! with its own writer and its own retained state:
//!
//! * `action_results` — per-tick drain of terminal verdicts (engine-owned).
//! * `action_stages` — bounded full transition history (own `HashMap`).
//! * `action_lifecycle` — collapsed `{in_flight, recent_terminal}` display
//!   view (a SECOND, parallel `HashMap` that mirrored every stage edge).
//! * publish-terminal status — the `publish_queue` entry status.
//!
//! The S11 convergence collapses these into ONE ledger keyed by
//! `correlation_id` that signing-return, publish, and cancel all record into,
//! with the prior surfaces becoming *derived projections* of the ledger rather
//! than parallel sources of truth (D4 — single writer of action state).
//!
//! # First slice (#1847): `action_lifecycle` derives from the ledger
//!
//! That slice resolved #1684: `action_lifecycle` is no longer an independent
//! store. The ledger owns the substrate stage history (the [`StageHistory`]
//! storage) plus the per-`correlation_id` curated failure reason code (#1735),
//! and the `action_lifecycle` projection is *computed* from that one record via
//! [`ActionLedger::lifecycle_snapshot`]. There is no second `HashMap`: the
//! `in_flight` / `recent_terminal` arrays are derived from the same per-stage
//! history the `action_stages` projection serialises, collapsed to the latest
//! stage per `correlation_id`.
//!
//! # Second slice (this change): `action_results` derives from the ledger
//!
//! `action_results` is the per-tick *drain* of terminal verdicts the host reads
//! to clear an action spinner. It used to be serialised from a PARALLEL source —
//! the publish engine's `pending_terminals` `Vec`, which every terminal-recording
//! path also pushed onto in addition to mirroring the stage into the ledger. That
//! made the engine `Vec` a SECOND source of truth for terminal verdicts.
//!
//! This slice inverts the source: the ledger is now the SINGLE writer of terminal
//! verdicts. Every terminal — an engine relay-settlement drained from
//! `pending_terminals`, a sign-step failure, a cancel, an off-band NWC success —
//! records into the ledger via [`ActionLedger::record_terminal`], which appends a
//! per-tick [`result_records::ActionResultRecord`] onto a drain buffer alongside
//! the stage write. `action_results` is serialised by draining that buffer
//! ([`ActionLedger::take_terminal_results`]). The off-band engine-side terminal
//! pushes are deleted: those verdicts now go straight to the ledger, not through
//! the engine `Vec`. The engine `pending_terminals` survives only as the inbound
//! transport for terminals that ORIGINATE asynchronously inside the engine (relay
//! ack/tick, NoTargets, cancel); the kernel drains that transport and records each
//! into the ledger, so the ledger is the one source the projection reads — there
//! is no parallel representation of a terminal verdict.
//!
//! # Third slice (#1758 slice 3): `action_stages` formalised as a derived facet
//!
//! The stage-history storage (renamed [`StageHistory`], `new()` test-only) is
//! documented as the ledger's facet, derived solely (byte-identically) via
//! [`ActionLedger::stages_snapshot`].
//!
//! The remaining surface (`publish_queue` per-`event_id` terminal status) is a
//! SEPARATE path keyed on `event_id` (the engine's `recently_completed` →
//! `take_completed` → `set_publish_entry_terminal` lane); it is collapsed in the
//! final slice and is untouched here. This change is byte-identical to the
//! prior `pending_terminals`-sourced `action_results` output (the same per-tick
//! drain semantics, the same `ok → published` status mapping, the same `error` /
//! `result` / `event_id` fields, and the same `reason_code` threading into the
//! lifecycle view).
//!
//! # D-doctrine
//!
//! * **D4** — one writer (the ledger); lifecycle is a derived view, not a
//!   second source of truth.
//! * **D6** — derivation is infallible; a serialization failure collapses to
//!   `Null` (the projection key is then omitted).
//! * **D8** — bounded by the inner [`StageHistory`] caps
//!   (`MAX_TRACKED_CORRELATIONS`, `MAX_STAGES_PER_CORRELATION`); the coded-reason
//!   sidecar is pruned in lock-step with the history so it can never outgrow it.
//! * **D9** — all wall-clock reads route through the caller-supplied `now_ms`
//!   (the kernel clock), keeping `FixedClock` tests deterministic.

pub(crate) mod result_records;

use result_records::ActionResultRecord;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
use super::action_stages::StageEntry;
use super::action_stages::{
    ActionStage, StageHistory, MAX_TRACKED_CORRELATIONS, PENDING_STAGE_RETENTION_MS,
    TERMINAL_STAGE_RETENTION_MS,
};

/// Retention window for terminal lifecycle entries. Mirrors the stage-history
/// terminal TTL so the derived lifecycle view and the substrate history expire
/// a settled row on the same edge. The ledger itself prunes via the inner
/// stage tracker's TTL; this alias documents the lifecycle-view boundary and is
/// consumed by the rung3 / projection_rev TTL-edge tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RECENT_TERMINAL_TTL_MS: u64 = TERMINAL_STAGE_RETENTION_MS;

/// Curated failure reason attached to a `correlation_id` (#1735). The
/// substrate `action_stages` history keeps only the English prose `reason`;
/// the machine-stable `code` (+ optional `subject`) rides the *derived*
/// `action_lifecycle` projection only, so a host can localize it. Stored on the
/// per-`correlation_id` latest-lifecycle slot rather than in the `ActionStage`
/// itself (the substrate stage type stays prose-only — its structured code is
/// S7's, #1754).
#[derive(Clone, Debug, Default)]
struct CodedReason {
    code: Option<String>,
    subject: Option<String>,
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
struct LatestLifecycle {
    /// Latest substrate stage observed for the id (collapsed from the history).
    stage: ActionStage,
    /// Wall-clock millis of that latest transition — the TTL retention anchor
    /// for the derived lifecycle view (terminal vs pending window).
    at_ms: u64,
    /// Curated failure reason (#1735) attached to the latest transition.
    coded: CodedReason,
}

impl LatestLifecycle {
    /// TTL window for this latest stage: short terminal TTL vs longer pending
    /// retention, mirroring the substrate history's per-stage retention.
    fn retention_ttl_ms(&self) -> u64 {
        if self.stage.is_terminal() {
            TERMINAL_STAGE_RETENTION_MS
        } else {
            PENDING_STAGE_RETENTION_MS
        }
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
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed { .. } | Self::Cancelled)
    }

    /// Derive a display stage from the latest substrate stage of a
    /// `correlation_id`, attaching the curated reason code (`Failed` only).
    fn derive(stage: &ActionStage, coded: &CodedReason) -> Self {
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

/// The single per-`correlation_id` record of action state.
///
/// Owns the substrate stage history ([`StageHistory`], the
/// `action_stages` source) plus the per-`correlation_id` latest-lifecycle slot
/// (the `action_lifecycle` source). Both projections are *derived views* over
/// this one record — there is no second tracker. The two facets share the same
/// key set: every mutation reconciles `latest` to the live history so the
/// latest-lifecycle map can never outgrow the D8-bounded history.
#[derive(Default)]
pub(crate) struct ActionLedger {
    /// Substrate stage-history facet — the bounded per-`correlation_id`
    /// transition log owning the eviction order, caps, and TTL for `action_stages`.
    stages: StageHistory,
    /// Latest lifecycle state per `correlation_id` — the source the derived
    /// `action_lifecycle` view collapses to. Updated on EVERY record (even when
    /// the history cap drops a non-terminal diagnostic row). Bounded by its own
    /// first-record order + global cap (`latest_order`), mirroring the deleted
    /// `ActionLifecycleTracker`'s latest-stage-wins semantics exactly.
    latest: HashMap<String, LatestLifecycle>,
    /// First-record order of the lifecycle facet's `correlation_ids`. Front-
    /// popped on global-cap overflow; the derived view walks it for stable
    /// ordering (a fresh dispatch lands at the bottom of the spinner list). The
    /// lifecycle facet owns its order independently of the history's so a
    /// record the history dropped at its per-correlation cap still keeps a
    /// stable lifecycle position.
    latest_order: Vec<String>,
    /// Per-tick terminal-verdict drain — the SINGLE source of `action_results`
    /// (S11 slice 2, #1758). Every terminal recorded via [`Self::record_terminal`]
    /// appends one [`ActionResultRecord`] here in producer order; the kernel
    /// serialises and clears it once per emit via [`Self::take_terminal_results`].
    /// Unbounded only WITHIN a tick (drained every emit), so it cannot grow
    /// across ticks — the same per-tick `Vec` lifetime the deleted engine
    /// `pending_terminals` had.
    terminal_results: Vec<ActionResultRecord>,
}

impl ActionLedger {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a stage transition for `correlation_id` with no curated reason
    /// code. The kernel's hot path goes through [`Self::record_coded`]; this
    /// un-coded convenience is exercised by the ledger unit tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn record(
        &mut self,
        correlation_id: &str,
        stage: ActionStage,
        detail: Option<serde_json::Value>,
        at_ms: u64,
    ) {
        self.record_coded(correlation_id, stage, detail, None, None, at_ms);
    }

    /// As [`Self::record`], but threads the curated failure `reason_code`
    /// (+ optional `reason_subject`, #1735) into the lifecycle-derivation
    /// sidecar for this `correlation_id`. The substrate history sees only the
    /// prose `reason`. A `reason_code` of `None` clears any prior code for the
    /// id (a later un-coded record overrides a stale curated one).
    pub(crate) fn record_coded(
        &mut self,
        correlation_id: &str,
        stage: ActionStage,
        detail: Option<serde_json::Value>,
        reason_code: Option<&str>,
        reason_subject: Option<&str>,
        at_ms: u64,
    ) {
        // The history records the prose-only stage (bounded; may drop a
        // non-terminal diagnostic at the per-correlation cap).
        self.stages
            .record(correlation_id, stage.clone(), detail, at_ms);
        // The latest-lifecycle slot ALWAYS advances on every record — it is the
        // authoritative lifecycle state and is independent of the history cap,
        // so a 65th non-terminal still moves the displayed stage + TTL anchor
        // (the deleted `ActionLifecycleTracker`'s latest-stage-wins contract).
        // Latest curated code wins; `None` clears any prior code.
        let is_new = !self.latest.contains_key(correlation_id);
        if is_new && self.latest.len() >= MAX_TRACKED_CORRELATIONS {
            // Global cap: evict the oldest lifecycle slot by first-record order,
            // mirroring `StageHistory`'s overflow semantics so the two
            // facets agree on which ids survive at cap.
            if let Some(oldest) = self.latest_order.first().cloned() {
                self.latest.remove(&oldest);
                self.latest_order.remove(0);
            }
        }
        self.latest.insert(
            correlation_id.to_string(),
            LatestLifecycle {
                stage,
                at_ms,
                coded: CodedReason {
                    code: reason_code.map(str::to_string),
                    subject: reason_subject.map(str::to_string),
                },
            },
        );
        if is_new {
            self.latest_order.push(correlation_id.to_string());
        }
    }

    /// Record a TERMINAL verdict for `correlation_id` — the single write that
    /// makes the ledger the source of `action_results` (S11 slice 2, #1758).
    ///
    /// Does two things in one call: (1) records the terminal `stage` into the
    /// ledger via [`Self::record_coded`] (so the `action_stages` history and the
    /// derived `action_lifecycle` view pick it up, with `reason_code` /
    /// `reason_subject` threaded), and (2) appends one [`ActionResultRecord`]
    /// onto the per-tick `terminal_results` drain the kernel serialises into
    /// `action_results`. `status` is the already-mapped WIRE status
    /// (`"published"` / `"failed"` / `"cancelled"`); `error` / `result_json` /
    /// `event_id` are the verbatim row fields.
    ///
    /// This is the ONE funnel every terminal-recording path routes through:
    /// engine relay-settlements (drained from the engine transport), sign-step
    /// failures, cancels, and off-band NWC successes. Each terminal appends
    /// exactly one row — so `action_results` carries one entry per terminal,
    /// drained once.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_terminal(
        &mut self,
        correlation_id: &str,
        stage: ActionStage,
        status: &'static str,
        error: Option<String>,
        result_json: Option<String>,
        event_id: Option<String>,
        reason_code: Option<&str>,
        reason_subject: Option<&str>,
        at_ms: u64,
    ) {
        // Record the stage first so the `action_stages` / `action_lifecycle`
        // projections observe the terminal in the same edge.
        self.record_coded(
            correlation_id,
            stage,
            None,
            reason_code,
            reason_subject,
            at_ms,
        );
        // Then enqueue the per-tick action_results row — the single source.
        self.terminal_results.push(ActionResultRecord {
            correlation_id: correlation_id.to_string(),
            status,
            error,
            result_json,
            event_id,
        });
    }

    /// Drain the per-tick terminal-verdict buffer into the `action_results` row
    /// array. Pure drain (D-review #29 semantics, now ledger-owned): each
    /// terminal appears exactly once, then is consumed — a later tick with no
    /// new terminal yields an empty `Vec`. Returns the rows in producer order.
    #[must_use]
    pub(crate) fn take_terminal_results(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.terminal_results)
            .iter()
            .map(ActionResultRecord::to_row)
            .collect()
    }

    /// Early-dismiss `correlation_id` from the ledger. Idempotent (D6). Drops
    /// the id from BOTH facets so the derived `action_stages` and
    /// `action_lifecycle` views clear it on the next emit.
    pub(crate) fn ack(&mut self, correlation_id: &str) -> bool {
        let removed_stage = self.stages.ack(correlation_id);
        let removed_latest = self.latest.remove(correlation_id).is_some();
        if removed_latest {
            if let Some(pos) = self.latest_order.iter().position(|id| id == correlation_id) {
                self.latest_order.remove(pos);
            }
        }
        removed_stage || removed_latest
    }

    /// Number of `correlation_ids` in the `action_stages` history facet. Used
    /// by the kernel's `action_stages` rev-bump (TTL-drop) edge detection.
    pub(crate) fn stages_entry_count(&self) -> usize {
        self.stages.entry_count()
    }

    /// Number of `correlation_ids` in the `action_lifecycle` (latest) facet.
    /// Used by the kernel's `action_lifecycle` rev-bump edge detection. The two
    /// facets can prune on different edges (a non-terminal dropped at the
    /// per-correlation history cap still advances the latest slot's TTL anchor),
    /// so each projection counts its own facet.
    pub(crate) fn lifecycle_entry_count(&self) -> usize {
        self.latest.len()
    }

    /// Derive the `action_stages` projection — the SINGLE serialisation of the
    /// ledger's [`StageHistory`] facet (bounded full history, no parallel
    /// tracker). Byte-identical to the prior output. Prunes expired rows (TTL).
    ///
    /// Does NOT touch the latest-lifecycle facet: the two facets prune on their
    /// own anchors (a record the history dropped at its per-correlation cap
    /// leaves the history's latest entry older than the latest-lifecycle slot,
    /// so the history may TTL-expire the id while the lifecycle slot is still
    /// live — exactly the prior two-tracker behaviour).
    pub(crate) fn stages_snapshot(&mut self, now_ms: u64) -> serde_json::Value {
        self.stages.snapshot(now_ms)
    }

    /// Derive the `action_lifecycle` display projection from the one ledger.
    ///
    /// Reads the per-`correlation_id` latest-lifecycle slot (the authoritative
    /// lifecycle state, advanced on every record regardless of the history
    /// cap), attaches the curated reason code (`Failed` only), and partitions
    /// into `{in_flight, recent_terminal}` in first-record order. Returns
    /// [`serde_json::Value::Null`] when both arrays would be empty so the
    /// projection helper omits the key (steady state).
    ///
    /// Output is byte-identical to the prior `ActionLifecycleTracker`: the
    /// latest stage per id (latest-stage-wins, INCLUDING a record the history
    /// dropped at its per-correlation cap), the same first-record ordering, the
    /// same terminal vs pending TTL, and the same curated `reason_code` /
    /// `reason_subject`.
    pub(crate) fn lifecycle_snapshot(&mut self, now_ms: u64) -> serde_json::Value {
        self.prune_latest(now_ms);
        if self.latest.is_empty() {
            return serde_json::Value::Null;
        }
        let mut in_flight: Vec<LifecycleEntry> = Vec::new();
        let mut recent_terminal: Vec<LifecycleEntry> = Vec::new();
        // Walk the lifecycle facet's own first-record order so the arrays carry
        // the same stable ordering the prior standalone tracker produced.
        for cid in &self.latest_order {
            let Some(latest) = self.latest.get(cid) else {
                continue;
            };
            let display = LifecycleStage::derive(&latest.stage, &latest.coded);
            let entry = LifecycleEntry {
                correlation_id: cid.clone(),
                stage: display.clone(),
            };
            if display.is_terminal() {
                recent_terminal.push(entry);
            } else {
                in_flight.push(entry);
            }
        }
        if in_flight.is_empty() && recent_terminal.is_empty() {
            return serde_json::Value::Null;
        }
        let payload = LifecycleSnapshot {
            in_flight,
            recent_terminal,
        };
        serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null)
    }

    /// Drop latest-lifecycle slots whose retention TTL has elapsed. Terminal
    /// slots use the short toast TTL; non-terminal slots use the longer pending
    /// TTL — mirroring the substrate history's per-stage retention, anchored at
    /// the slot's own latest `at_ms`. Keeps `latest_order` in sync.
    fn prune_latest(&mut self, now_ms: u64) {
        let mut drop_ids: Vec<String> = Vec::new();
        for (cid, l) in &self.latest {
            if now_ms >= l.at_ms.saturating_add(l.retention_ttl_ms()) {
                drop_ids.push(cid.clone());
            }
        }
        if drop_ids.is_empty() {
            return;
        }
        for cid in &drop_ids {
            self.latest.remove(cid);
        }
        self.latest_order.retain(|cid| !drop_ids.contains(cid));
    }

    /// Test/diagnostic accessor: number of tracked correlation_ids (lifecycle
    /// facet — the authoritative latest-stage map).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.latest.len()
    }

    /// Test-only: per-correlation stage history (for parity assertions).
    #[cfg(test)]
    pub(crate) fn history(&self, correlation_id: &str) -> Option<&[StageEntry]> {
        self.stages.history(correlation_id)
    }
}

#[cfg(test)]
#[path = "action_ledger/tests.rs"]
mod tests;
