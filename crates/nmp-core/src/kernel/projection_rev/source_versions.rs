//! ADR-0055 Rung 1 — typed source-version counter struct.
//!
//! `SourceVersions` holds one named `u64` counter per distinct source domain.
//! Counters are bumped at the SINGLE write chokepoint for that domain (D4
//! discipline — same sites that already bump `changed_since_emit`).
//!
//! ## Why a typed struct, not a HashMap
//!
//! The ADR spec (option C, codex-validated) says "a TYPED struct of named u64
//! counters (NOT a HashMap)". Advantages:
//! - Zero heap allocation; all counters live inline in the `Kernel` struct.
//! - The dependency table references counter names as `&'static str`; the
//!   `get()` method maps names to struct fields via a match arm (exhaustive,
//!   compiler-enforced). Adding a new source counter without updating `get()`
//!   causes a compile error, not a silent 0.
//! - Mirrors the Bevy `Mut<T>` seqlock pattern: a single-field bump is one
//!   atomic-width operation on the actor thread (no lock, no hash lookup).
//!
//! ## Bump discipline (D8, no polling)
//!
//! Every bump site is a write chokepoint called from the actor thread as a
//! direct consequence of a state mutation — never in a timer, never in a
//! polling loop. Bumps are O(1) `u64::saturating_add(1)`.

use super::{
    SRC_ACCOUNTS, SRC_ACTIVE_ACCOUNT, SRC_CLAIMED_EVENT_CONTENT, SRC_CONFIGURED_RELAYS,
    SRC_DIAGNOSTICS_INPUTS, SRC_OPEN_VIEWS, SRC_PROFILE_CLAIMS, SRC_PROFILES, SRC_PUBLISH,
    SRC_SETTLEMENT_DRAIN, SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY,
};

/// Typed source-version counters for the Tier-2 built-in projections.
///
/// All fields default to 0. Reset to 0 on `Kernel` rebuild (the Reset path
/// constructs a new `Kernel`; `SourceVersions::default()` handles it).
#[derive(Default, Debug, Clone)]
pub(crate) struct SourceVersions {
    // ── identity cluster ──────────────────────────────────────────────────────
    /// Bumped at `ingest_profile` (the write chokepoint for kind:0 profile
    /// metadata — called after `verify_and_persist` returns `Inserted|Replaced`
    /// AND the new profile supersedes the cached one).
    pub(crate) profiles_ver: u64,

    /// Bumped at `set_accounts` / `set_active_account` (the sole writers of
    /// `Kernel::accounts` / `Kernel::active_account` — D4: actor is sole writer).
    pub(crate) accounts_ver: u64,

    /// Bumped at `set_accounts` / `set_active_account` / `set_active_account_for_test`
    /// whenever the active-account pubkey changes. Separate from `accounts_ver`
    /// so `active_account` (a scalar) and `profile` (which reads the active
    /// account's kind:0) can gate independently.
    pub(crate) active_account_ver: u64,

    // ── profile/event claim cluster ───────────────────────────────────────────
    /// Bumped at `claim_profile` / `release_profile` (the sole writers of
    /// `Kernel::profile_claims` — D4 via `requests/profile.rs`).
    pub(crate) profile_claims_ver: u64,

    /// Bumped on three conditions (codex #1 — store-backed + enrichment):
    /// 1. `claim_event` / `release_event` (the sole writers of
    ///    `Kernel::event_claims` — D4 via `requests/event.rs`).
    /// 2. A store-insert/replace whose event-id OR addressable coord matches a
    ///    live `event_claims` key — checked at the `verify_and_persist`
    ///    chokepoint in `ingest/`.
    /// 3. `profiles_ver` bumps AND `event_claims` is non-empty — the enrichment
    ///    dependency: `claimed_events` joins author kind:0 display/picture, so a
    ///    profile update for an author of a live claimed event must re-derive the
    ///    projection.
    pub(crate) claimed_event_content_ver: u64,

    /// Bumped when `open_views` changes. Currently always-empty (V-112/ADR-0042
    /// deleted author_view/thread_view). Still declared so a future view-open
    /// populating `mention_profiles` triggers a rev bump.
    pub(crate) open_views_ver: u64,

    // ── relay/settings cluster ────────────────────────────────────────────────
    /// Bumped at `set_configured_relays` (the sole PRODUCTION writer of
    /// `Kernel::configured_relays` — D4, `identity_state.rs`). The test-only
    /// `clear_configured_relays_for_test` does not bump (a fresh kernel / Reset
    /// rebuild zeroes the tracker, so no explicit reset bump is needed).
    pub(crate) configured_relays_ver: u64,

    // ── publish cluster ───────────────────────────────────────────────────────
    /// Bumped at every publish-queue write chokepoint (`identity_state.rs`):
    /// - `push_publish_entry` (enqueue a new publish intent)
    /// - `remove_publish_entry` (drop an entry)
    /// - `set_publish_entry_terminal` (terminal `ok` / `failed` transition)
    ///
    /// The `publish_queue` is the single source of truth: `publish_outbox` and
    /// `outbox_summary` are derived read-only views over it, so they ride
    /// `publish_ver` and need no separate write chokepoint.
    pub(crate) publish_ver: u64,

    // ── diagnostics cluster (broad stamp, sub-fork A) ─────────────────────────
    /// Bumped at the write chokepoint of EVERY input that feeds
    /// `relay_diagnostics_snapshot()` (codex #4, sub-fork A):
    /// relay status/health transitions, relay role changes, transport-relay
    /// additions/removals, wire-sub open/close, logical-interest open/close,
    /// profile_claims changes (profile_claims_ver also bumps this),
    /// active-account change (active_account_ver also bumps this),
    /// profile-cache updates that feed relay-diagnostics (profiles_ver also bumps
    /// this), mailbox/cache coverage changes, configured-relays changes
    /// (configured_relays_ver also bumps this), lifecycle status transitions.
    ///
    /// The "also bumps" pattern ensures the broad stamp is a superset of the
    /// narrow per-domain stamps — a relay_diagnostics consumer is never stale
    /// relative to any of its inputs.
    pub(crate) diagnostics_inputs_ver: u64,

    // ── drain + TTL projections ───────────────────────────────────────────────
    /// Bumped at the settlement-enqueue chokepoint:
    /// - `record_action_stage` (stages/lifecycle enqueue)
    /// - `take_action_results_projection` / `take_signed_events_projection`
    ///   (drain path — presence rules: Changed when non-empty, Cleared when empty)
    pub(crate) settlement_enqueue_ver: u64,

    /// Bumped when a drain (`action_results`, `signed_events`) is actually
    /// consumed — i.e. the tick where `take_*_projection` returns non-Null.
    /// Used together with `settlement_enqueue_ver` to let the presence rule
    /// distinguish Changed (non-empty drain) from Cleared (empty drain).
    pub(crate) settlement_drain_ver: u64,

    /// Bumped when `action_lifecycle.prune_expired` actually removes a row
    /// (codex #3 — wall-clock TTL-expiry edge, D8-compliant: no separate timer,
    /// called from the existing emit/ingest edge). Stable on idle ticks where
    /// no row crosses its deadline.
    pub(crate) ttl_expiry_ver: u64,
}

impl SourceVersions {
    /// Return the value of the named counter. Returns 0 for unknown names
    /// (an unknown name indicates a stale dependency table — caught by tests).
    pub(crate) fn get(&self, name: &str) -> u64 {
        match name {
            SRC_PROFILES => self.profiles_ver,
            SRC_ACCOUNTS => self.accounts_ver,
            SRC_ACTIVE_ACCOUNT => self.active_account_ver,
            SRC_PROFILE_CLAIMS => self.profile_claims_ver,
            SRC_CLAIMED_EVENT_CONTENT => self.claimed_event_content_ver,
            SRC_OPEN_VIEWS => self.open_views_ver,
            SRC_CONFIGURED_RELAYS => self.configured_relays_ver,
            SRC_PUBLISH => self.publish_ver,
            SRC_DIAGNOSTICS_INPUTS => self.diagnostics_inputs_ver,
            SRC_SETTLEMENT_ENQUEUE => self.settlement_enqueue_ver,
            SRC_SETTLEMENT_DRAIN => self.settlement_drain_ver,
            SRC_TTL_EXPIRY => self.ttl_expiry_ver,
            _ => 0,
        }
    }

    /// Bump `profiles_ver`. (relay_diagnostics is covered by the per-emit
    /// fingerprint reconcile, F5 — no co-bump needed here.)
    pub(crate) fn bump_profiles(&mut self) {
        self.profiles_ver = self.profiles_ver.saturating_add(1);
    }

    /// Bump `accounts_ver`.
    pub(crate) fn bump_accounts(&mut self) {
        self.accounts_ver = self.accounts_ver.saturating_add(1);
    }

    /// Bump `active_account_ver`.
    pub(crate) fn bump_active_account(&mut self) {
        self.active_account_ver = self.active_account_ver.saturating_add(1);
    }

    /// Bump `profile_claims_ver`.
    pub(crate) fn bump_profile_claims(&mut self) {
        self.profile_claims_ver = self.profile_claims_ver.saturating_add(1);
    }

    /// Bump `claimed_event_content_ver`.
    pub(crate) fn bump_claimed_event_content(&mut self) {
        self.claimed_event_content_ver = self.claimed_event_content_ver.saturating_add(1);
    }

    /// Bump `open_views_ver`.
    pub(crate) fn bump_open_views(&mut self) {
        self.open_views_ver = self.open_views_ver.saturating_add(1);
    }

    /// Bump `configured_relays_ver`.
    pub(crate) fn bump_configured_relays(&mut self) {
        self.configured_relays_ver = self.configured_relays_ver.saturating_add(1);
    }

    /// Bump `publish_ver`.
    pub(crate) fn bump_publish(&mut self) {
        self.publish_ver = self.publish_ver.saturating_add(1);
    }

    /// Bump `diagnostics_inputs_ver`. Sole caller is the per-emit
    /// `reconcile_diagnostics_fingerprint` (F5): the broad `relay_diagnostics`
    /// stamp is derived from a fingerprint of the projection's own encoded bytes,
    /// so it advances iff any of its many inputs (relay status, wire subs,
    /// interests) actually changed — no per-site stamping, no missed input.
    pub(crate) fn bump_diagnostics_inputs(&mut self) {
        self.diagnostics_inputs_ver = self.diagnostics_inputs_ver.saturating_add(1);
    }

    /// Bump `settlement_enqueue_ver`.
    pub(crate) fn bump_settlement_enqueue(&mut self) {
        self.settlement_enqueue_ver = self.settlement_enqueue_ver.saturating_add(1);
    }

    /// Bump `settlement_drain_ver`.
    pub(crate) fn bump_settlement_drain(&mut self) {
        self.settlement_drain_ver = self.settlement_drain_ver.saturating_add(1);
    }

    /// Bump `ttl_expiry_ver`.
    pub(crate) fn bump_ttl_expiry(&mut self) {
        self.ttl_expiry_ver = self.ttl_expiry_ver.saturating_add(1);
    }
}
