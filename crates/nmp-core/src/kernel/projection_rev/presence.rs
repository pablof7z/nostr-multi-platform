//! ADR-0055 Rung 3 S1b — presence state-machine implementations.
//!
//! Extracted from `mod.rs` (which was approaching the 500-LOC hard ceiling)
//! to make room for `note_copy_emit` (the Cleared-edge machine for the
//! copy-with-TTL keys `action_stages` / `action_lifecycle` — §10.4 of
//! `docs/decisions/0055-rung3.md`).
//!
//! # What lives here
//!
//! - `note_drain_emit` — the `Changed → Cleared → Unchanged` tristate for the
//!   two true drain keys (`action_results`, `signed_events`). Unchanged from
//!   Rung 1.
//! - `note_copy_emit` — the **new** analogous edge machine for the two
//!   copy-with-TTL keys (`action_stages`, `action_lifecycle`). Same
//!   state-machine shape, but bumps `ttl_expiry_ver` on the Cleared edge
//!   (not `settlement_drain_ver`) to advance the rev.
//! - `presence_for` — the lookup that `build_manifest` / `build_state` call
//!   to classify each key for the current tick.
//!
//! # Cleared-edge safety (§10.3 — exactly once, edge not level)
//!
//! Both machines write into `pending_presence[key]` once per emit tick.
//! `record_emitted` (in `mod.rs`) clears the entry, so `presence_for` returns
//! `Unchanged` on the NEXT tick — fire once on the edge, then settle.
//! `record_emitted_for_manifest` (`rung2_stamp.rs`) iterates the FULL key
//! universe including the Cleared key (manifest always covers all 18 keys),
//! so `last_emitted[cleared_key]` advances on the Cleared tick — the next
//! tick's rev-vs-last-emit check sees Unchanged and no synthesis occurs.

use super::{static_key, ProjectionPresence, ProjectionRevTracker};

impl ProjectionRevTracker {
    /// Record a drain-projection emit and return its presence for THIS tick
    /// (the `Changed → Cleared → Unchanged` tristate). Called from the drain
    /// chokepoint (`take_action_results_projection` /
    /// `take_signed_events_projection`) EXACTLY ONCE per emit per drain key,
    /// with `nonempty` = "the drain carried content this tick".
    ///
    /// State machine (ADR-0055 codex #2 — Cleared is emitted exactly once on
    /// the non-empty → empty transition so the host drops its prior copy
    /// without a replay, and a stably-empty drain settles to Unchanged):
    ///
    /// - `nonempty`                  → bump `settlement_drain_ver`; `Changed`
    /// - `!nonempty` && was nonempty → bump `settlement_drain_ver`; `Cleared`
    /// - `!nonempty` && was empty    → NO bump; `Unchanged`
    ///
    /// The presence is parked in `pending_presence` for `build_manifest` to
    /// read and the `drain_prev_nonempty` content state is updated for the
    /// next tick.
    pub(crate) fn note_drain_emit(&mut self, key: &str, nonempty: bool) -> ProjectionPresence {
        let Some(static_key) = static_key(key) else {
            return ProjectionPresence::Unchanged;
        };
        let was_nonempty = self
            .drain_prev_nonempty
            .get(static_key)
            .copied()
            .unwrap_or(false);
        let presence = if nonempty {
            self.source_versions.bump_settlement_drain();
            ProjectionPresence::Changed
        } else if was_nonempty {
            // non-empty → empty transition: advance the rev once so the
            // Cleared frame is distinguishable, then settle.
            self.source_versions.bump_settlement_drain();
            ProjectionPresence::Cleared
        } else {
            // stably empty: no bump, no churn.
            ProjectionPresence::Unchanged
        };
        self.drain_prev_nonempty.insert(static_key, nonempty);
        self.pending_presence.insert(static_key, presence);
        presence
    }

    /// Record a copy-with-TTL projection emit and return its presence for
    /// THIS tick. Analogous to `note_drain_emit` but for the two keys whose
    /// tracker is copied (not drained) each tick:
    /// `action_stages` and `action_lifecycle`.
    ///
    /// State machine (§10.4 of `docs/decisions/0055-rung3.md`):
    ///
    /// - `nonempty`                  → `Changed` (no extra bump — the rev
    ///                                  already moved on enqueue/expiry)
    /// - `!nonempty` && was nonempty → bump `ttl_expiry_ver` so the rev
    ///                                  advances on the Cleared frame; `Cleared`
    /// - `!nonempty` && was empty    → `Unchanged`
    ///
    /// This ensures that `ack_action_stage` removing the last entry →
    /// next emit sees `was_nonempty=true`, snapshot Null → `Cleared` presence
    /// → §10.2 synthesis emits the Cleared row → host drops the stale stage.
    /// Without this machine the post-ack manifest presence would stay
    /// `Unchanged` and the host would retain the stale row forever.
    ///
    /// Called once per emit inside `action_stages_projection` and
    /// `action_lifecycle_projection` (`publish_cmd.rs`).
    pub(crate) fn note_copy_emit(&mut self, key: &str, nonempty: bool) -> ProjectionPresence {
        let Some(static_key) = static_key(key) else {
            return ProjectionPresence::Unchanged;
        };
        let was_nonempty = self
            .copy_prev_nonempty
            .get(static_key)
            .copied()
            .unwrap_or(false);
        let presence = if nonempty {
            ProjectionPresence::Changed
        } else if was_nonempty {
            // non-empty → empty transition: bump ttl_expiry_ver so the rev
            // advances, making the Cleared frame distinguishable from the
            // prior Changed frame (same advance discipline as note_drain_emit).
            self.source_versions.bump_ttl_expiry();
            ProjectionPresence::Cleared
        } else {
            ProjectionPresence::Unchanged
        };
        self.copy_prev_nonempty.insert(static_key, nonempty);
        self.pending_presence.insert(static_key, presence);
        presence
    }

    /// Compute the presence for `key` this tick.
    ///
    /// Keys with a `pending_presence` override (drain or copy-with-TTL keys
    /// whose `note_drain_emit` / `note_copy_emit` was called this tick) return
    /// the parked value. All other keys use the rev-vs-last-emit rule:
    /// `Changed` when the rev advanced, else `Unchanged`.
    pub(crate) fn presence_for(&self, key: &'static str) -> ProjectionPresence {
        if let Some(p) = self.pending_presence.get(key) {
            return *p;
        }
        if self.changed_since_last_emit(key) {
            ProjectionPresence::Changed
        } else {
            ProjectionPresence::Unchanged
        }
    }
}
