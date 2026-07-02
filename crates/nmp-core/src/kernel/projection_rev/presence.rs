//! ADR-0070 Rung 3 S1b — presence state-machine implementations.
//!
//! Extracted from `mod.rs` (which was approaching the 500-LOC hard ceiling)
//! to make room for `note_copy_emit` (the Cleared-edge machine for the
//! copy-with-TTL keys `action_stages` / `action_lifecycle` — §10.4 of
//! `docs/decisions/0070-typed-read-sessions.md`).
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

use crate::kernel::snapshot_registry::DeclaredProjections;
use crate::kernel::update::KERNEL_BUILTIN_PROJECTION_KEYS;

use super::{static_key, ProjectionPresence, ProjectionRevTracker};

impl ProjectionRevTracker {
    /// Reconcile the host-declared consumed-projection gate with the per-key
    /// presence machine for this tick.
    ///
    /// A key that is filtered out by the declaration gate must not be classified
    /// as manifest-`Changed` just because it has never been emitted; it is absent
    /// by policy, not because the producer missed a row. Conversely, a key that
    /// was previously filtered out and is newly permitted must emit a baseline
    /// row even when its logical payload is empty and no source-version counter
    /// changed. Otherwise the host cache observes an absent->present cache-unit
    /// transition while the manifest says `Unchanged`, which is the #1430 oracle
    /// failure and would be omitted under incremental apply.
    pub(crate) fn reconcile_declared_permits(&mut self, declared: &DeclaredProjections) {
        for &key in KERNEL_BUILTIN_PROJECTION_KEYS {
            let permitted = declared.permits(key);
            match self.last_declared_permits.insert(key, permitted) {
                Some(false) if permitted => {
                    self.pending_presence
                        .insert(key, ProjectionPresence::Changed);
                }
                Some(true) if !permitted => {
                    self.pending_presence
                        .insert(key, ProjectionPresence::Cleared);
                }
                _ if !permitted => {
                    self.pending_presence
                        .insert(key, ProjectionPresence::Unchanged);
                }
                _ => {}
            }
        }
    }

    /// Record a drain-projection emit and return its presence for THIS tick
    /// (the `Changed → Cleared → Unchanged` tristate). Called from the drain
    /// chokepoint (`take_action_results_projection` /
    /// `take_signed_events_projection`) EXACTLY ONCE per emit per drain key,
    /// with `nonempty` = "the drain carried content this tick".
    ///
    /// State machine (ADR-0070 codex #2 — Cleared is emitted exactly once on
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
    /// State machine (§10.4 of `docs/decisions/0070-typed-read-sessions.md`).
    ///
    /// CRITICAL — unlike `note_drain_emit`, the copy-with-TTL keys
    /// (`action_stages` / `action_lifecycle`) PERSIST across ticks: a stable
    /// in-flight action keeps the same content every 4Hz tick. So this machine
    /// is **edge-only**: it parks `pending_presence` ONLY on the
    /// non-empty → empty (`Cleared`) edge. The non-empty steady state is left
    /// to the normal rev-vs-last-emit rule in `presence_for`, so a tick whose
    /// content genuinely did not change settles to `Unchanged` and is omitted
    /// from the frame (the whole point of Rung 3). If we parked `Changed` here
    /// unconditionally, every stable tick would re-emit the full payload
    /// forever — a regression vs master on the exact path the ADR optimizes
    /// (#1390 review FIX 1).
    ///
    /// Per-content-change advance is the responsibility of the source-version
    /// bump at the mutation site: `ack_action_stage` /
    /// `enqueue` / TTL-expiry bump `settlement_enqueue_ver` / `ttl_expiry_ver`,
    /// which `presence_for` reads via `changed_since_last_emit`.
    ///
    /// - `nonempty`                  → `Changed` (informational return; does
    ///                                  NOT park presence — rev rule governs)
    /// - `!nonempty` && was nonempty → bump `ttl_expiry_ver`; park `Cleared`
    ///                                  (the one edge this machine injects)
    /// - `!nonempty` && was empty    → `Unchanged` (informational; not parked)
    ///
    /// This ensures that `ack_action_stage` removing the last entry →
    /// next emit sees `was_nonempty=true`, snapshot Null → parks `Cleared`
    /// presence → §10.2 synthesis emits the Cleared row → host drops the stale
    /// stage. Without this edge the post-ack manifest presence would stay
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
        self.copy_prev_nonempty.insert(static_key, nonempty);
        if nonempty {
            // Steady-state non-empty: do NOT park presence. Leave the key to
            // the rev-vs-last-emit rule so an unchanged tick resolves to
            // Unchanged (and is omitted). Content changes are signalled by the
            // source-version bump at the mutation site (e.g. ack_action_stage).
            ProjectionPresence::Changed
        } else if was_nonempty {
            // non-empty → empty transition: bump ttl_expiry_ver so the rev
            // advances, making the Cleared frame distinguishable from the
            // prior frame, and park Cleared so §10.2 synthesises the row.
            self.source_versions.bump_ttl_expiry();
            self.pending_presence
                .insert(static_key, ProjectionPresence::Cleared);
            ProjectionPresence::Cleared
        } else {
            // stably empty: no bump, no park, no churn.
            ProjectionPresence::Unchanged
        }
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
