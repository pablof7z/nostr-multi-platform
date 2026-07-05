//! `ActionLedger` — the single per-`correlation_id` record of action state.
//! # Why this exists (S11, #1758 / #1684)
//! Action outcomes used to be recorded into FOUR overlapping surfaces, each
//! with its own writer and its own retained state:
//! * `action_results` — per-tick drain of terminal verdicts (engine-owned).
//! * `action_stages` — bounded full transition history (own `HashMap`).
//! * `action_lifecycle` — collapsed `{in_flight, recent_terminal}` display
//!   view (a SECOND, parallel `HashMap` that mirrored every stage edge).
//! * publish-terminal status — the `publish_queue` entry status.
//! The S11 convergence collapses these into ONE ledger keyed by
//! `correlation_id` that signing-return, publish, and cancel all record into,
//! with the prior surfaces becoming *derived projections* of the ledger rather
//! than parallel sources of truth (D4 — single writer of action state).
//! # First slice (#1847): `action_lifecycle` derives from the ledger
//! That slice resolved #1684: `action_lifecycle` is no longer an independent
//! store. The ledger owns the substrate stage history (the [`StageHistory`]
//! storage) plus the per-`correlation_id` curated failure reason code (#1735),
//! and the `action_lifecycle` projection is *computed* from that one record via
//! [`ActionLedger::lifecycle_snapshot`]. There is no second `HashMap`: the
//! `in_flight` / `recent_terminal` arrays are derived from the same per-stage
//! history the `action_stages` projection serialises, collapsed to the latest
//! stage per `correlation_id`.
//! # Second slice (this change): `action_results` derives from the ledger
//! `action_results` is the per-tick *drain* of terminal verdicts the host reads
//! to clear an action spinner. It used to be serialised from a PARALLEL source —
//! the publish engine's `pending_terminals` `Vec`, which every terminal-recording
//! path also pushed onto in addition to mirroring the stage into the ledger. That
//! made the engine `Vec` a SECOND source of truth for terminal verdicts.
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

// The `action_lifecycle` display-view data types (`LifecycleStage` /
// `LifecycleEntry` / `LifecycleSnapshot` / the private `CodedReason` /
// `LatestLifecycle`) live in this submodule — split out to keep this file
// under the AGENTS.md LOC cap. Re-exported at this path so existing callers
// (including the test suite) see no path change.
mod lifecycle_view;
use lifecycle_view::{CodedReason, LatestLifecycle, LifecycleStage};
pub use lifecycle_view::{LifecycleEntry, LifecycleSnapshot};

use result_records::ActionResultRecord;
use std::collections::HashMap;

#[cfg(test)]
use super::action_stages::StageEntry;
use super::action_stages::{
    ActionStage, StageHistory, MAX_TRACKED_CORRELATIONS, TERMINAL_STAGE_RETENTION_MS,
};

/// Retention window for terminal lifecycle entries, counted from FIRST
/// OBSERVATION (chirp#115) rather than from the transition instant — see
/// [`lifecycle_view::LatestLifecycle::observed_terminal_at_ms`] and
/// [`ActionLedger::prune_latest`]. Mirrors the stage-history terminal TTL
/// value so the derived lifecycle view and the substrate history use the same
/// numeric window (their eviction ANCHORS now differ by design: the history
/// facet still anchors on the transition instant, since it is a diagnostic
/// surface, not the load-bearing spinner path). This alias documents the
/// lifecycle-view boundary and is consumed by the rung3 / projection_rev
/// TTL-edge tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RECENT_TERMINAL_TTL_MS: u64 = TERMINAL_STAGE_RETENTION_MS;

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
                // Every record — including a re-record of a previously terminal
                // id (e.g. a retry that starts a new lifecycle) — starts
                // unobserved. `lifecycle_snapshot` marks this the moment it
                // actually serves the entry as a `recent_terminal` row.
                observed_terminal_at_ms: None,
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
    /// Output is byte-identical to the prior `ActionLifecycleTracker` on the
    /// happy path (same first-record ordering, same curated `reason_code` /
    /// `reason_subject`), with one deliberate behaviour change (chirp#115): a
    /// terminal row is now retained until THIS call has actually served it at
    /// least once — see [`Self::prune_latest`] — rather than aging out on
    /// wall-clock distance from the original transition alone.
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
            let Some(latest) = self.latest.get_mut(cid) else {
                continue;
            };
            let display = LifecycleStage::derive(&latest.stage, &latest.coded);
            let entry = LifecycleEntry {
                correlation_id: cid.clone(),
                stage: display.clone(),
            };
            if display.is_terminal() {
                // First time this terminal is actually served in a snapshot:
                // start the post-observation grace window from HERE. Until
                // this line runs, `prune_latest` treats the entry as
                // unconditionally retained (chirp#115) — a slow consumer can
                // never race the terminal out of existence before seeing it.
                if latest.observed_terminal_at_ms.is_none() {
                    latest.observed_terminal_at_ms = Some(now_ms);
                }
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

    /// Drop latest-lifecycle slots whose retention has elapsed. Keeps
    /// `latest_order` in sync.
    ///
    /// Non-terminal slots are unchanged: they age out
    /// [`PENDING_STAGE_RETENTION_MS`] after their latest `at_ms`, mirroring the
    /// substrate history's per-stage retention.
    ///
    /// Terminal slots (chirp#115 fix) are NOT time-boxed relative to the
    /// transition instant. A terminal entry that has never been served by
    /// [`ActionLedger::lifecycle_snapshot`] (`observed_terminal_at_ms.is_none()`)
    /// is exempt from TTL pruning here — it is retained no matter how long the
    /// relay round-trip or the emit cadence took, so a consumer can never
    /// observe an empty `recent_terminal` for an action it is still waiting
    /// on. Once observed at least once, [`TERMINAL_STAGE_RETENTION_MS`] bounds
    /// how much longer it lingers, anchored to the observation instant rather
    /// than the original transition — the terminal TTL is now a POST-DELIVERY
    /// display grace window, not a delivery deadline. An id that is never
    /// observed at all is still bounded: `record_coded`'s global
    /// [`MAX_TRACKED_CORRELATIONS`] drop-oldest cap is the sole backstop, the
    /// same one already relied on for a host that never acks.
    fn prune_latest(&mut self, now_ms: u64) {
        let mut drop_ids: Vec<String> = Vec::new();
        for (cid, l) in &self.latest {
            let expired = if l.stage.is_terminal() {
                match l.observed_terminal_at_ms {
                    None => false,
                    Some(observed_at_ms) => {
                        now_ms >= observed_at_ms.saturating_add(TERMINAL_STAGE_RETENTION_MS)
                    }
                }
            } else {
                l.pending_ttl_expired(now_ms)
            };
            if expired {
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
