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
//!    through the dependency table (max of deps = derived rev, monotonic).
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
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod oracle;
#[cfg(test)]
mod tests;

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
}

impl ProjectionRevTracker {
    /// Return the current derived revision for `key`.
    ///
    /// The derived rev is the maximum source-version among all of `key`'s
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

    /// Record that `key` was emitted at the current derived rev.
    /// Used by the test-support oracle (Rung 1) and Rung 3 producer logic.
    pub(crate) fn record_emitted(&mut self, key: &str) {
        // Map key str to a &'static str from KERNEL_BUILTIN_PROJECTION_KEYS for
        // the HashMap key (avoids allocating a String key per call).
        if let Some(static_key) = KERNEL_BUILTIN_PROJECTION_KEYS
            .iter()
            .copied()
            .find(|k| *k == key)
        {
            let rev = self.compute_rev(static_key);
            self.last_emitted.insert(static_key, rev);
        }
    }

    /// Return `true` if the projection's derived rev advanced since the last
    /// recorded emit.
    pub(crate) fn changed_since_last_emit(&self, key: &str) -> bool {
        let current = self.compute_rev(key);
        let last = self.last_emitted.get(key).copied().unwrap_or(0);
        current > last
    }

    /// Bump the epoch. Called on account-switch / schema-change / kernel rebuild.
    /// The next emit MUST be a full baseline (all projections -> `Changed`).
    pub(crate) fn bump_epoch(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
    }
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
        .map(|key| {
            let rev = tracker.projection_rev(key);
            let presence = if tracker.changed_since_last_emit(key) {
                ProjectionPresence::Changed
            } else {
                ProjectionPresence::Unchanged
            };
            ProjectionState {
                key,
                rev,
                presence,
            }
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
        Some(static_key) => {
            let rev = tracker.projection_rev(static_key);
            let presence = if tracker.changed_since_last_emit(static_key) {
                ProjectionPresence::Changed
            } else {
                ProjectionPresence::Unchanged
            };
            ProjectionState {
                key: static_key,
                rev,
                presence,
            }
        }
        None => ProjectionState {
            key: "unknown",
            rev: 0,
            presence: ProjectionPresence::Unchanged,
        },
    }
}
