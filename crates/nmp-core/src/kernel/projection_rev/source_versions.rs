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

use std::collections::HashMap;

use super::{
    SRC_ACCOUNTS, SRC_ACTIVE_ACCOUNT, SRC_CLAIMED_EVENT_CONTENT, SRC_CONFIGURED_RELAYS,
    SRC_DIAGNOSTICS_INPUTS, SRC_OPEN_VIEWS, SRC_PROFILES, SRC_PROFILE_CLAIMS, SRC_PUBLISH,
    SRC_PUBLISH_ENGINE, SRC_REF_EVENT_ROWS, SRC_REF_PROFILE_ROWS, SRC_SETTLEMENT_DRAIN,
    SRC_SETTLEMENT_ENQUEUE, SRC_TTL_EXPIRY,
};
use crate::kernel::refs::RefNamespace;

/// Typed source-version counters for the Tier-2 built-in projections.
///
/// All fields default to 0. Reset to 0 on `Kernel` rebuild (the Reset path
/// constructs a new `Kernel`; `SourceVersions::default()` handles it).
#[derive(Default, Debug, Clone)]
pub(crate) struct SourceVersions {
    // ── identity cluster ──────────────────────────────────────────────────────
    /// Bumped by the ingest chokepoint when an accepted kind:0 supersedes the
    /// cached profile (ADR-0057 PR 2: the registered `nmp_nip01::Kind0Parser`
    /// writes the capability-owned `ProfileCache` inside `verify_and_persist`;
    /// the chokepoint detects the before/after cache transition and bumps this).
    /// Also bumped when RAM eviction removes a cached profile.
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
    /// Bumped at `resolve_ref` / `release_ref` (the sole writers of
    /// `Kernel::profile_claims` — D4 via `requests/profile.rs`).
    pub(crate) profile_claims_ver: u64,

    /// Bumped on two conditions (codex #1 — store-backed claimed content):
    /// 1. event `resolve_ref` / `release_ref` (the sole writers of
    ///    `Kernel::event_claims` — D4 via `requests/event.rs`).
    /// 2. A store-insert/replace whose event-id OR addressable coord matches a
    ///    live `event_claims` key — checked at the `verify_and_persist`
    ///    chokepoint in `ingest/`.
    pub(crate) claimed_event_content_ver: u64,

    /// Bumped when `open_views` changes. Currently always-empty (V-112/ADR-0042
    /// deleted author_view/thread_view). Still declared so a future view-open
    /// driving the profile resolve path (`refs.profile`) triggers a rev bump.
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
    pub(crate) publish_ver: u64,

    /// Bumped when the embedded `PublishEngine`'s in-flight snapshot changes.
    /// `publish_outbox` and `outbox_summary` derive from this engine state, not
    /// from `Kernel::publish_queue`, so they need a separate source stamp.
    pub(crate) publish_engine_ver: u64,

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

    /// Bumped when action feedback TTL pruning actually removes a row
    /// (`action_lifecycle` or the legacy `action_stages` mirror). This is the
    /// wall-clock TTL-expiry edge: D8-compliant, no separate timer, called from
    /// the existing emit/snapshot edge. Stable on idle ticks where no row
    /// crosses its deadline.
    pub(crate) ttl_expiry_ver: u64,

    // ── ADR-0063 (#1671 Lane B): per-KEY ref-row revisions ────────────────────
    /// Per-KEY revision for `refs.profile` rows (keyed by raw hex pubkey). The
    /// whole-projection `profile_claims_ver` scalar above stays the ADR-0055
    /// manifest source until Lane A migrates it; THIS map is the row-grain source
    /// of truth ADR-0063 D6a needs (only the changed pubkey's row crosses FFI).
    /// Bumped at three sites: resolve (`resolve_profile_ref`), release
    /// (`release_profile_ref`), and the kind:0 ingest chokepoint
    /// (`project_accepted_event`, gated on a live claim). Monotonic; reset only
    /// on `Kernel` rebuild.
    ///
    /// ## Bounded-cleanup lifecycle (BLOCKING 2 fix)
    ///
    /// An entry is created ONLY when a row is actually mutated: every call site
    /// gates the bump on a real claim / real refcount change / live-claimed
    /// ingest, so a spurious release of a never-claimed key never inserts a row
    /// (see the gated `bump_*_row` callers). When a row is fully released its
    /// last-release teardown calls [`Self::clear_profile_row`], which bumps the
    /// rev to its final post-clear value (so the `Cleared` row a downstream
    /// emitter produces carries the monotonic value) and then **immediately
    /// removes the entry in the same call** — there is no retained-rev / pending
    /// state, so the map is always bounded to currently-claimed keys (D8). An
    /// explicit `Cleared` resets the host cache entry (ADR-0055 §D1), so a later
    /// re-resolve starts a fresh row lifetime at rev 1 — monotonicity only has to
    /// hold while a row is live between `Changed` and `Cleared`.
    pub(crate) profile_row_revs: HashMap<String, u64>,
    /// Per-KEY revision for `refs.event` rows (keyed by `primary_id`: hex64 id or
    /// `kind:pubkey:d` coord). Event twin of [`Self::profile_row_revs`]; bumped at
    /// resolve/release (`requests/event.rs`) and the store-ingest chokepoint
    /// (`maybe_bump_claimed_event_content`, already gated on a live claim). Same
    /// bounded-cleanup lifecycle as `profile_row_revs`.
    pub(crate) event_row_revs: HashMap<String, u64>,

    // ── ADR-0063 (#1671 integration glue): whole-projection ref-row stamps ─────
    /// Monotonic whole-projection stamp for `refs.profile`. Co-bumped inside
    /// every per-KEY profile-row mutation chokepoint ([`Self::bump_profile_row`]
    /// / [`Self::clear_profile_row`]) so the derived `refs.profile` projection rev
    /// advances whenever ANY profile row mutates. Monotonic across release: unlike
    /// summing `profile_row_revs` (which a clear shrinks by removing the entry),
    /// this scalar only ever increases, so the manifest rev never regresses.
    pub(crate) ref_profile_rows_ver: u64,
    /// Event twin of [`Self::ref_profile_rows_ver`] for `refs.event`.
    pub(crate) ref_event_rows_ver: u64,
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
            SRC_PUBLISH_ENGINE => self.publish_engine_ver,
            SRC_DIAGNOSTICS_INPUTS => self.diagnostics_inputs_ver,
            SRC_SETTLEMENT_ENQUEUE => self.settlement_enqueue_ver,
            SRC_SETTLEMENT_DRAIN => self.settlement_drain_ver,
            SRC_TTL_EXPIRY => self.ttl_expiry_ver,
            SRC_REF_PROFILE_ROWS => self.ref_profile_rows_ver,
            SRC_REF_EVENT_ROWS => self.ref_event_rows_ver,
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

    /// Bump `publish_engine_ver`.
    pub(crate) fn bump_publish_engine(&mut self) {
        self.publish_engine_ver = self.publish_engine_ver.saturating_add(1);
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

    /// ADR-0063 (#1671 Lane B) — bump the per-KEY rev for one `refs.profile` row.
    ///
    /// Callers MUST gate this on an actual row mutation (a real claim, a real
    /// refcount change, a shape-widen / liveness-upgrade, or a live-claimed
    /// ingest) so the map stays bounded to claimed keys — a spurious or no-op bump
    /// for an unchanged key would create / advance a row with nothing on the wire
    /// to carry it (BLOCKING 2, BLOCKING 3).
    pub(crate) fn bump_profile_row(&mut self, key: &str) {
        let rev = self.profile_row_revs.entry(key.to_string()).or_insert(0);
        *rev = rev.saturating_add(1);
        // ADR-0063 integration glue: co-bump the whole-projection stamp so the
        // derived `refs.profile` manifest rev advances on any row mutation
        // (intrinsic — no separate call site to forget).
        self.ref_profile_rows_ver = self.ref_profile_rows_ver.saturating_add(1);
    }

    /// ADR-0063 (#1671 Lane B) — bump the per-KEY rev for one `refs.event` row.
    /// Same gating contract as [`Self::bump_profile_row`].
    pub(crate) fn bump_event_row(&mut self, key: &str) {
        let rev = self.event_row_revs.entry(key.to_string()).or_insert(0);
        *rev = rev.saturating_add(1);
        // ADR-0063 integration glue: co-bump the whole-projection stamp.
        self.ref_event_rows_ver = self.ref_event_rows_ver.saturating_add(1);
    }

    /// ADR-0063 (#1671 Lane B) — final-`Cleared` teardown of one ref row's per-key
    /// rev (BLOCKING 2). Called from the last-release / terminal-miss teardown
    /// AFTER the consumer state is gone. It bumps the rev to its final post-clear
    /// value (the value an ADR-0055 `Cleared` row carries) and **immediately
    /// removes the entry in the same call** — there is no retained-rev or pending
    /// state, so the map never accumulates released keys (D8: memory scales with
    /// active views, not history). Returns the final rev so a row-delta emitter
    /// can stamp the `Cleared` frame; on this branch (no Lane A emitter yet) the
    /// caller discards it.
    ///
    /// Ordering (documented per BLOCKING 2): `bump → (emit Cleared with final
    /// rev) → remove`, all in the same tick. With no in-branch emitter the middle
    /// step is elided and the rev is dropped immediately after the bump. An
    /// explicit `Cleared` resets the host cache entry (ADR-0055 §D1), so a later
    /// re-resolve legitimately starts a fresh row at rev 1.
    ///
    /// No-op (returns 0) when the key has no rev entry — a never-claimed key never
    /// reached the `Cleared` state, so a spurious release does not create a row.
    pub(crate) fn clear_profile_row(&mut self, key: &str) -> u64 {
        if !self.profile_row_revs.contains_key(key) {
            return 0;
        }
        self.bump_profile_row(key);
        let final_rev = self.profile_row_revs.get(key).copied().unwrap_or(0);
        self.profile_row_revs.remove(key);
        final_rev
    }

    /// Event twin of [`Self::clear_profile_row`].
    pub(crate) fn clear_event_row(&mut self, key: &str) -> u64 {
        if !self.event_row_revs.contains_key(key) {
            return 0;
        }
        self.bump_event_row(key);
        let final_rev = self.event_row_revs.get(key).copied().unwrap_or(0);
        self.event_row_revs.remove(key);
        final_rev
    }

    /// ADR-0063 (#1671 Lane B) — read the per-KEY rev for `(namespace, key)`.
    /// Returns 0 for an unseen key (a row that has never resolved).
    pub(crate) fn ref_row_rev(&self, namespace: RefNamespace, key: &str) -> u64 {
        match namespace {
            RefNamespace::Profile => self.profile_row_revs.get(key).copied().unwrap_or(0),
            RefNamespace::Event => self.event_row_revs.get(key).copied().unwrap_or(0),
        }
    }
}
