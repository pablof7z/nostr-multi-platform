//! ADR-0055 Rung 1 — kernel-owned per-projection revision manifest.
//!
//! # Rung 1 scope
//!
//! This module implements the revision manifest exactly as specified: the kernel
//! tracks a monotonic `u64` revision per Tier-2 built-in projection key, derived
//! from named `SourceVersions` counters bumped at each input's WRITE CHOKEPOINT.
//!
//! **Rung 1 is pure infrastructure.** It does NOT change wire bytes — `make_update`
//! does NOT consult the manifest yet. The manifest is the source of truth that
//! Rung 2 stamps onto the wire and Rung 3 uses to omit Unchanged projections.
//!
//! ## Why `allow(dead_code)` on the manifest derivation
//!
//! In a PURE production build (no `test` / `test-support`) the manifest's only
//! consumer — the biconditional oracle — is `cfg`-compiled out, so the rev
//! derivation (`compute_rev`, `build_manifest`, the `SRC_*` names) reads as dead
//! even though it is live under test AND is the Rung 2/3 deliverable that the
//! wire encoder will consume next. The `SourceVersions::bump_*` write chokepoints
//! ARE live in production (called from ingest / identity / publish / relay
//! mutation sites). The allow is scoped to this module so a genuinely-unused new
//! kernel symbol elsewhere is still caught.
#![allow(dead_code)]
//!
//! ## Design (option C — source-version stamps + derived rev)
//!
//! Validated by opus+codex review. NOT per-mutation-site-per-projection bumping,
//! NOT content-hash-as-gate. Instead:
//!
//! 1. A small typed struct `SourceVersions` holds one named `u64` counter per
//!    distinct source domain. Counters are bumped at the SINGLE write chokepoint
//!    for that domain (D4 discipline — same discipline as `changed_since_emit`).
//! 2. A `BUILTIN_PROJECTION_DEPENDENCIES` table (const) declares which source
//!    counters each projection key depends on.
//! 3. `ProjectionRevTracker` derives per-key revs by folding source counters
//!    through the dependency table (SUM of deps = derived rev, monotonic).
//!
//! ## Correctness: co-location enforcement
//!
//! Every Tier-2 built-in key MUST appear in `BUILTIN_PROJECTION_DEPENDENCIES` or
//! the `all_builtin_keys_have_dependency_entries` test fails at compile time.
//! A new key added to `KERNEL_BUILTIN_PROJECTION_KEYS` without a corresponding
//! dependency entry is caught at `cargo test -p nmp-core` time.
//!
//! ## Presence rules (codex #2)
//!
//! - Steady-state keyed projections: `Changed` when rev advanced since last emit,
//!   else `Unchanged`.
//! - Drain projections (`action_results`, `signed_events`): `Changed` when drained
//!   non-empty this tick; `Cleared` when empty this tick (explicit, NEVER
//!   `Unchanged` — prevents stale one-shot replay in Rung 3).
//! - Copy-with-TTL (`action_stages`, `action_lifecycle`): `Changed` on
//!   enqueue-or-real-expiry; `Cleared` the tick the tracker is empty;
//!   `Unchanged` while holding rows unchanged.

pub(crate) mod source_versions;
// ADR-0055 Rung 1: the `impl Kernel` manifest accessors live in a sibling file
// so `kernel/mod.rs` stays at its file-size baseline.
mod kernel_impl;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod oracle;
// ADR-0055 Rung 1 (F3): the `impl Kernel` oracle methods live in a sibling file
// (test-support only) so `kernel/mod.rs` stays at its file-size baseline.
#[cfg(any(test, feature = "test-support"))]
mod kernel_oracle;
#[cfg(test)]
mod tests;
// ADR-0055 Rung 1: scenario tests live in `tests.rs`; the dependency-table
// completeness + arithmetic unit tests live here to keep each test file under
// the 500-LOC hard ceiling (AGENTS.md). Both share the same crate-private API.
#[cfg(test)]
mod tests_unit;

use crate::kernel::update::KERNEL_BUILTIN_PROJECTION_KEYS;
pub(crate) use source_versions::SourceVersions;

// ── Public types ──────────────────────────────────────────────────────────────

/// The presence-state of a projection in this tick's manifest.
///
/// Wire encoding (Rung 2 will stamp these onto the frame):
/// - `Changed`: rev advanced since last emit; payload PRESENT.
/// - `Unchanged`: rev identical to last emit; payload OMITTED (host reuses cache).
/// - `Cleared`: the projection went absent this tick (e.g. drain emptied, view
///   closed). Payload omitted. NEVER conflated with `Unchanged` — prevents the
///   classic delta-protocol footgun where absence is ambiguous with clearing
///   (ADR-0055 D3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionPresence {
    Changed,
    Unchanged,
    Cleared,
}

/// Per-projection revision state in the manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectionState {
    /// The canonical projection key (one of `KERNEL_BUILTIN_PROJECTION_KEYS`).
    pub(crate) key: &'static str,
    /// Monotonically non-decreasing revision for this projection. Reset to 0
    /// on epoch bump (account-switch / schema-change / kernel rebuild).
    pub(crate) rev: u64,
    /// Presence classification for this tick.
    pub(crate) presence: ProjectionPresence,
}

/// The complete per-tick revision manifest for all Tier-2 kernel built-in
/// projection keys.
///
/// Created by `Kernel::projection_manifest()` and readable via
/// `Kernel::projection_state(key)`. In Rung 1, this is internal-only; Rung 2
/// stamps the data onto the wire.
#[derive(Clone, Debug)]
pub(crate) struct ProjectionManifest {
    /// Kernel-start wall-clock stamp (`TimingMilestones::started_unix_ms`).
    /// Reused rather than adding new state (ADR-0055 D4 decision). A host
    /// detects "this is a new kernel run" when `session_id` changes.
    pub(crate) session_id: u64,
    /// Within-session monotonic counter. Bumped on epoch-class events:
    /// account-switch, schema-change, kernel rebuild (the `Kernel::Reset` path).
    /// On bump, the next emit is a full baseline (all projections -> `Changed`).
    pub(crate) epoch: u64,
    /// Per-key state for every Tier-2 built-in. Ordered by
    /// `KERNEL_BUILTIN_PROJECTION_KEYS` index for stable iteration.
    pub(crate) states: Vec<ProjectionState>,
}

// ── Dependency table ──────────────────────────────────────────────────────────

/// Source counter names used in `BUILTIN_PROJECTION_DEPENDENCIES`.
pub(crate) const SRC_PROFILES: &str = "profiles_ver";
pub(crate) const SRC_ACTIVE_ACCOUNT: &str = "active_account_ver";
pub(crate) const SRC_ACCOUNTS: &str = "accounts_ver";
pub(crate) const SRC_PROFILE_CLAIMS: &str = "profile_claims_ver";
pub(crate) const SRC_CLAIMED_EVENT_CONTENT: &str = "claimed_event_content_ver";
pub(crate) const SRC_OPEN_VIEWS: &str = "open_views_ver";
pub(crate) const SRC_CONFIGURED_RELAYS: &str = "configured_relays_ver";
pub(crate) const SRC_PUBLISH: &str = "publish_ver";
pub(crate) const SRC_DIAGNOSTICS_INPUTS: &str = "diagnostics_inputs_ver";
pub(crate) const SRC_SETTLEMENT_ENQUEUE: &str = "settlement_enqueue_ver";
pub(crate) const SRC_SETTLEMENT_DRAIN: &str = "settlement_drain_ver";
pub(crate) const SRC_TTL_EXPIRY: &str = "ttl_expiry_ver";

/// Per-key source-counter dependency list (Rung 1 dependency map).
///
/// Each entry is `(projection_key, &[source_counter_name, ...])`.
/// Every key in `KERNEL_BUILTIN_PROJECTION_KEYS` MUST have an entry here.
/// The `all_builtin_keys_have_dependency_entries` test asserts this.
pub(crate) const BUILTIN_PROJECTION_DEPENDENCIES: &[(&str, &[&str])] = &[
    // identity cluster
    ("profile",          &[SRC_PROFILES, SRC_ACTIVE_ACCOUNT]),
    ("accounts",         &[SRC_ACCOUNTS, SRC_PROFILES]),
    ("active_account",   &[SRC_ACTIVE_ACCOUNT]),
    // profile/event claim cluster
    ("claimed_profiles", &[SRC_PROFILE_CLAIMS, SRC_PROFILES]),
    ("resolved_profiles",&[SRC_PROFILE_CLAIMS, SRC_PROFILES]),
    // claimed_event_content_ver: bumped on (1) claim_event/release_event,
    // (2) store-ingest that matches a live claim, (3) profiles_ver bump when
    // event_claims is non-empty (enrichment dependency, codex #1).
    ("claimed_events",   &[SRC_CLAIMED_EVENT_CONTENT]),
    // mention_profiles: always-empty today (V-112/ADR-0042), but open_views_ver
    // is declared so any future view-open populating it triggers a rev bump.
    ("mention_profiles", &[SRC_OPEN_VIEWS]),
    // relay/settings cluster — all depend on configured_relays_ver
    ("configured_relays",&[SRC_CONFIGURED_RELAYS]),
    ("relay_role_options",&[SRC_CONFIGURED_RELAYS]),
    ("settings_hub",     &[SRC_CONFIGURED_RELAYS]),
    // publish cluster
    ("publish_queue",    &[SRC_PUBLISH]),
    ("publish_outbox",   &[SRC_PUBLISH]),
    ("outbox_summary",   &[SRC_PUBLISH]),
    // drain projections: settlement-enqueue + DRAIN presence rule (codex #2).
    // settlement_drain_ver bumped when a drain returns non-empty (Changed) or
    // empty (Cleared). The rev still advances on enqueue.
    ("action_results",   &[SRC_SETTLEMENT_ENQUEUE, SRC_SETTLEMENT_DRAIN]),
    ("signed_events",    &[SRC_SETTLEMENT_ENQUEUE, SRC_SETTLEMENT_DRAIN]),
    // copy-with-TTL: settlement-enqueue + wall-clock TTL-expiry edge (codex #3).
    ("action_stages",    &[SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY]),
    ("action_lifecycle", &[SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY]),
    // relay_diagnostics: broad diagnostics_inputs_ver (sub-fork A, codex #4).
    // One broad stamp covers: relay status/health, transport info, wire.subs,
    // logical interests, profile_claims, active account, profile cache,
    // mailbox/cache coverage, configured_relays, lifecycle status.
    ("relay_diagnostics",&[SRC_DIAGNOSTICS_INPUTS]),
];

// ── Revision tracker ──────────────────────────────────────────────────────────

/// The per-projection revision tracker owned by `Kernel`.
///
/// Holds the source-version counters (`SourceVersions`) and the derived per-key
/// revision state. Tracks the last-emitted rev for each key so callers can ask
/// whether a projection changed since the last emit.
///
/// Reset to zero on `Kernel` rebuild (the `Reset` path constructs a fresh
/// `Kernel`, so a new `ProjectionRevTracker::default()` on `Kernel::new` is
/// free — no explicit reset logic is needed).
#[derive(Default)]
pub(crate) struct ProjectionRevTracker {
    /// Named source-version counters bumped at each domain's write chokepoint.
    pub(crate) source_versions: SourceVersions,
    /// Per-key last-emitted revision. Updated by `record_emitted`.
    last_emitted: std::collections::HashMap<&'static str, u64>,
    /// Within-session monotonic epoch counter.
    pub(crate) epoch: u64,
    /// Per-drain-key content state at the LAST emit (`true` = the drain carried
    /// content). Used to compute the `Changed -> Cleared -> Unchanged` tristate
    /// for the `action_results` / `signed_events` drain projections (codex #2,
    /// F2). A key absent here is treated as "previously empty".
    drain_prev_nonempty: std::collections::HashMap<&'static str, bool>,
    /// This-tick presence OVERRIDE for keys whose presence cannot be derived from
    /// the rev alone — the drain projections. `note_drain_emit` writes here during
    /// the emit; `build_manifest` / `build_state` consult it before falling back
    /// to the rev-vs-last-emit rule; `record_emitted` clears the entry once the
    /// emit is recorded so the next tick starts fresh.
    pending_presence: std::collections::HashMap<&'static str, ProjectionPresence>,
    /// ADR-0055 Rung 1 (F5): fingerprint of the `relay_diagnostics` inputs at the
    /// LAST reconcile. The kernel re-fingerprints them each emit and folds any
    /// change into `diagnostics_inputs_ver` — see `reconcile_diagnostics_fingerprint`.
    last_seen_diagnostics_fingerprint: u64,
}

/// The two drain projections whose presence is a `Changed -> Cleared ->
/// Unchanged` tristate driven by `note_drain_emit`, not by the rev alone.
pub(crate) const DRAIN_PROJECTION_KEYS: &[&str] = &["action_results", "signed_events"];

impl ProjectionRevTracker {
    /// Return the current derived revision for `key`.
    ///
    /// The derived rev is the SUM of source-versions across all of `key`'s
    /// declared dependencies. Returns 0 for an unknown key.
    pub(crate) fn projection_rev(&self, key: &str) -> u64 {
        self.compute_rev(key)
    }

    fn compute_rev(&self, key: &str) -> u64 {
        let deps = BUILTIN_PROJECTION_DEPENDENCIES
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, deps)| *deps)
            .unwrap_or(&[]);
        // Use saturating_add fold (sum) so that ANY dep bump advances the rev,
        // even when two deps reach the same counter value. Using max() would
        // silently stall the rev when e.g. ttl_expiry_ver catches up with
        // settlement_enqueue_ver (ADR-0055 codex #3 correctness — scenario S4).
        deps.iter()
            .map(|dep_name| self.source_versions.get(dep_name))
            .fold(0u64, |acc, v| acc.saturating_add(v))
    }

    /// Record a drain-projection emit and return its presence for THIS tick
    /// (F2 — the real tristate). Called from the drain chokepoint
    /// (`take_action_results_projection` / `take_signed_events_projection`)
    /// EXACTLY ONCE per emit per drain key, with `nonempty` = "the drain carried
    /// content this tick".
    ///
    /// State machine (codex #2 — Cleared is emitted exactly once on the
    /// non-empty -> empty transition so the host drops its prior copy without a
    /// replay, and a stably-empty drain settles to Unchanged):
    /// - `nonempty`                 -> bump `settlement_drain_ver`; `Changed`
    /// - `!nonempty` & was nonempty -> bump `settlement_drain_ver`; `Cleared`
    /// - `!nonempty` & was empty    -> NO bump; `Unchanged`
    ///
    /// The presence is parked in `pending_presence` for `build_manifest` to read
    /// and the `drain_prev_nonempty` content state is updated for the next tick.
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
            // non-empty -> empty transition: advance the rev once so the Cleared
            // frame is distinguishable, then settle.
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

    /// Record that `key` was emitted at the current derived rev.
    /// Clears any `pending_presence` override so the next tick starts fresh.
    pub(crate) fn record_emitted(&mut self, key: &str) {
        if let Some(static_key) = static_key(key) {
            let rev = self.compute_rev(static_key);
            self.last_emitted.insert(static_key, rev);
            self.pending_presence.remove(static_key);
        }
    }

    /// Return `true` if the projection's derived rev advanced since the last
    /// recorded emit. For drain keys, this also returns true when an explicit
    /// `Changed` / `Cleared` presence is pending this tick.
    ///
    /// Crucially: a key **absent** from `last_emitted` (i.e. never emitted, or
    /// cleared by `reset_last_emitted` / `bump_epoch`) is treated as `Changed`
    /// regardless of the current rev. This handles the case where the rev is
    /// still 0 (no mutations since kernel init) — the key must still be emitted
    /// in any full-baseline frame (D3-5).
    pub(crate) fn changed_since_last_emit(&self, key: &str) -> bool {
        // A pending explicit presence (drain keys) takes precedence.
        if let Some(static_key) = static_key(key) {
            if let Some(p) = self.pending_presence.get(static_key) {
                return matches!(p, ProjectionPresence::Changed | ProjectionPresence::Cleared);
            }
        }
        let current = self.compute_rev(key);
        match self.last_emitted.get(key).copied() {
            // Never emitted (or last_emitted cleared by reset/epoch bump) →
            // always Changed, even when rev is still 0.
            None => true,
            // Previously emitted at `last`; Changed only when rev advanced.
            Some(last) => current > last,
        }
    }

    /// Compute the presence for `key` this tick.
    ///
    /// Drain keys use the parked `pending_presence` (the `note_drain_emit` state
    /// machine). All other keys use the rev-vs-last-emit rule: `Changed` when the
    /// rev advanced, else `Unchanged`.
    fn presence_for(&self, key: &'static str) -> ProjectionPresence {
        if let Some(p) = self.pending_presence.get(key) {
            return *p;
        }
        if self.changed_since_last_emit(key) {
            ProjectionPresence::Changed
        } else {
            ProjectionPresence::Unchanged
        }
    }

    /// Bump the epoch. Called on account-switch / schema-change / kernel rebuild.
    /// The next emit MUST be a full baseline (all projections -> `Changed`).
    ///
    /// ADR-0055 Rung 3 (D3-5): also clears the per-key `last_emitted` tracker
    /// so every live Tier-2 projection is classified `Changed` on the next
    /// `make_update` tick, guaranteeing the mandatory full baseline frame.
    pub(crate) fn bump_epoch(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        // D3-5: clear the emitted-rev baseline so the NEXT frame is a full
        // baseline (all projections Changed). This is correct whether or not
        // incremental-apply is enabled — the epoch bump always signals a new
        // session context that requires a full re-baseline.
        self.last_emitted.clear();
    }

    /// ADR-0055 Rung 3 (D3-5) — reset the per-key emitted-rev baseline so the
    /// NEXT `make_update` frame is a full baseline (every live Tier-2
    /// projection emitted as `Changed`).
    ///
    /// Called when `declare_incremental_apply()` is set (the first host-attach
    /// after advertising incremental-apply must see a complete picture) and on
    /// `bump_epoch()` (above). Does NOT bump the epoch counter — only clears
    /// the `last_emitted` rev map so `presence_for` returns `Changed` for every
    /// key on the next tick.
    pub(crate) fn reset_last_emitted(&mut self) {
        self.last_emitted.clear();
    }

    /// ADR-0055 Rung 1 (F5): reconcile `diagnostics_inputs_ver` against a
    /// per-emit fingerprint of the EXACT `relay_diagnostics` inputs (relay
    /// statuses + wire subs + interests + transport rows). Called once per emit
    /// before the manifest is built.
    ///
    /// `relay_diagnostics` aggregates so many high-frequency inputs across so
    /// many mutation sites (relay status transitions, per-event sub counters,
    /// the interest registry's push/withdraw/ensure/drop across discovery,
    /// cache-serve, contacts, startup, claim-expansion, …) that enumerating every
    /// write chokepoint is fragile and demonstrably leaky. Sub-fork A mandates
    /// ONE broad stamp that covers ALL inputs; deriving that stamp from a
    /// fingerprint of the projection's own inputs is the only way to guarantee
    /// completeness with no missed site. This is a monotonic-stamp DERIVATION for
    /// the single broad diagnostic surface — NOT the rejected "content-hash as the
    /// Changed/Unchanged GATE for every projection" (the rev is still the
    /// authority; this only advances it when the inputs truly differ).
    ///
    /// Idempotent when the fingerprint is unchanged (stable relay state → no
    /// bump → `relay_diagnostics` settles to Unchanged, no churn).
    pub(crate) fn reconcile_diagnostics_fingerprint(&mut self, fingerprint: u64) {
        if fingerprint != self.last_seen_diagnostics_fingerprint {
            self.last_seen_diagnostics_fingerprint = fingerprint;
            self.source_versions.bump_diagnostics_inputs();
        }
    }
}

/// Resolve a string key to its `&'static str` from `KERNEL_BUILTIN_PROJECTION_KEYS`.
fn static_key(key: &str) -> Option<&'static str> {
    KERNEL_BUILTIN_PROJECTION_KEYS
        .iter()
        .copied()
        .find(|k| *k == key)
}

// ── Free functions (helpers for `impl Kernel`) ────────────────────────────────

/// Build the full `ProjectionManifest` for the current tick.
///
/// Every Tier-2 built-in key gets a `ProjectionState` entry. Presence is
/// `Changed` when the key's derived rev advanced since last emit, else
/// `Unchanged`. In Rung 1, this is internal-only; Rung 3 will use it to omit
/// Unchanged projections from the wire.
pub(crate) fn build_manifest(
    tracker: &ProjectionRevTracker,
    session_id: u64,
) -> ProjectionManifest {
    let states: Vec<ProjectionState> = KERNEL_BUILTIN_PROJECTION_KEYS
        .iter()
        .map(|key| ProjectionState {
            key,
            rev: tracker.projection_rev(key),
            presence: tracker.presence_for(key),
        })
        .collect();
    ProjectionManifest {
        session_id,
        epoch: tracker.epoch,
        states,
    }
}

/// Return the `ProjectionState` for a single key, or `Unchanged` at rev 0 if
/// the key is unknown.
pub(crate) fn build_state(tracker: &ProjectionRevTracker, key: &str) -> ProjectionState {
    let found: Option<&'static str> = KERNEL_BUILTIN_PROJECTION_KEYS
        .iter()
        .copied()
        .find(|k| *k == key);
    match found {
        Some(static_key) => ProjectionState {
            key: static_key,
            rev: tracker.projection_rev(static_key),
            presence: tracker.presence_for(static_key),
        },
        None => ProjectionState {
            key: "unknown",
            rev: 0,
            presence: ProjectionPresence::Unchanged,
        },
    }
}
