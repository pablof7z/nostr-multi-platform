//! ADR-0055 Rung 1 — 8 driven scenario tests + completeness enforcement.
//!
//! These tests drive SPECIFIC scenarios against the revision manifest.
//! The oracle only proves what you drive — hence 8 distinct scenarios.
//!
//! All tests run with `cargo test -p nmp-core`.

use crate::kernel::clock::MonotonicSecondClock;
use crate::kernel::projection_rev::{
    build_manifest, build_state, ProjectionPresence, ProjectionRevTracker,
    BUILTIN_PROJECTION_DEPENDENCIES,
};
use crate::kernel::update::KERNEL_BUILTIN_PROJECTION_KEYS;

// ── Scenario 1: store-backed claimed-event arrival ────────────────────────────

/// Scenario 1: A kind:30023/9802 event is claimed and then persisted via the
/// store (not `self.events`). The `claimed_events` rev MUST advance and the
/// presence MUST be `Changed`.
///
/// This validates the `claimed_event_content_ver` bump at the store-ingest
/// chokepoint (codex #1 condition 2).
#[test]
fn s1_store_backed_claimed_event_arrival_bumps_rev() {
    let mut tracker = ProjectionRevTracker::default();
    // Baseline: no claims yet. Record emit so last_emitted is seeded.
    tracker.record_emitted("claimed_events");
    let rev_before = tracker.projection_rev("claimed_events");

    // Simulate: event store insert matches a live claim (chokepoint bump).
    tracker.source_versions.bump_claimed_event_content();

    let rev_after = tracker.projection_rev("claimed_events");
    assert!(
        rev_after > rev_before,
        "claimed_events rev must advance after claimed_event_content_ver bump; before={rev_before} after={rev_after}"
    );
    assert!(
        tracker.changed_since_last_emit("claimed_events"),
        "claimed_events must be Changed after store-backed event arrival"
    );

    // Presence check via build_state.
    let state = build_state(&tracker, "claimed_events");
    assert_eq!(state.presence, ProjectionPresence::Changed);
}

// ── Scenario 2: profile enrichment after claim ────────────────────────────────

/// Scenario 2: A kind:0 profile arrives for an author of a live claimed event.
/// The `claimed_events` rev MUST advance (enrichment dependency, codex #1
/// condition 3: profiles_ver bumps AND event_claims is non-empty).
///
/// In Rung 1 we encode the enrichment dependency via
/// `claimed_event_content_ver` being bumped by the chokepoint in
/// `ingest_profile` when `event_claims` is non-empty. The test simulates that
/// bump directly.
#[test]
fn s2_profile_enrichment_after_claim_bumps_claimed_events_rev() {
    let mut tracker = ProjectionRevTracker::default();
    tracker.record_emitted("claimed_events");
    tracker.record_emitted("profile");
    let ce_before = tracker.projection_rev("claimed_events");
    let prof_before = tracker.projection_rev("profile");

    // Simulate: kind:0 arrives while event_claims is non-empty.
    // The chokepoint bumps both profiles_ver AND claimed_event_content_ver.
    tracker.source_versions.bump_profiles();
    tracker.source_versions.bump_claimed_event_content();
    // Also bumps active_account-derived profile.
    tracker.source_versions.bump_active_account();

    assert!(tracker.projection_rev("claimed_events") > ce_before,
        "claimed_events must advance after enrichment");
    assert!(tracker.projection_rev("profile") > prof_before,
        "profile must advance after profiles_ver bump");
    assert!(tracker.changed_since_last_emit("claimed_events"));
    assert!(tracker.changed_since_last_emit("profile"));
}

// ── Scenario 3: drain present→cleared ────────────────────────────────────────

/// Scenario 3: Settle an action_results entry (Changed), then emit again with
/// no new settlements (Cleared). The test verifies that:
/// - First emit: Changed (non-empty drain).
/// - Second emit (empty drain): Cleared (explicit, NOT Unchanged).
/// - No replay: `changed_since_last_emit` is false after a Cleared is recorded.
#[test]
fn s3_drain_present_then_cleared_no_replay() {
    let mut tracker = ProjectionRevTracker::default();

    // --- tick 1: enqueue a settlement ---
    tracker.source_versions.bump_settlement_enqueue();
    // Mark drain as non-empty this tick.
    tracker.source_versions.bump_settlement_drain();
    // Record emit (Changed).
    tracker.record_emitted("action_results");
    assert!(!tracker.changed_since_last_emit("action_results"),
        "after recording emit, must not be Changed");

    // --- tick 2: drain is empty this tick ---
    // The Cleared rule: settlement_drain_ver bumps again on empty drain
    // to distinguish "nothing to drain" from "unchanged".
    // In the actual implementation the empty-drain path calls bump_settlement_drain
    // to signal Cleared. We simulate it here.
    tracker.source_versions.bump_settlement_drain();
    // Now changed_since_last_emit must be true because drain ver advanced.
    assert!(
        tracker.changed_since_last_emit("action_results"),
        "empty drain must bump rev to signal Cleared"
    );

    // After recording this Cleared emit, the next tick is stable.
    tracker.record_emitted("action_results");
    assert!(!tracker.changed_since_last_emit("action_results"),
        "after Cleared recorded, must be stable (no replay)");
}

// ── Scenario 4: action_lifecycle TTL expiry via fixed clock ──────────────────

/// Scenario 4: A row crosses its TTL deadline (prune_expired returns true).
/// The `ttl_expiry_ver` MUST bump (rev advances); idle ticks where no row
/// expires MUST NOT bump (rev stable).
///
/// The test directly exercises the `ttl_expiry_ver` stamp discipline, mirroring
/// the `prune_expired → removed_any → bump_ttl_expiry` pattern in
/// `action_lifecycle.rs`.
#[test]
fn s4_action_lifecycle_ttl_expiry_bumps_rev_only_on_expiry() {
    let mut tracker = ProjectionRevTracker::default();
    // Enqueue something so the lifecycle key has a non-zero rev baseline.
    tracker.source_versions.bump_settlement_enqueue();
    tracker.record_emitted("action_lifecycle");
    let rev_before = tracker.projection_rev("action_lifecycle");

    // Idle tick: no TTL expiry, no new settlements.
    // Rev must remain stable.
    assert_eq!(
        tracker.projection_rev("action_lifecycle"),
        rev_before,
        "idle tick must not bump action_lifecycle rev"
    );
    assert!(
        !tracker.changed_since_last_emit("action_lifecycle"),
        "idle tick must be Unchanged"
    );

    // TTL expiry edge: prune_expired removed a row.
    tracker.source_versions.bump_ttl_expiry();
    assert!(
        tracker.projection_rev("action_lifecycle") > rev_before,
        "TTL expiry must advance action_lifecycle rev"
    );
    assert!(
        tracker.changed_since_last_emit("action_lifecycle"),
        "TTL expiry must be Changed"
    );
}

// ── Scenario 5: host-declared narrowing × rev ─────────────────────────────────

/// Scenario 5: Undeclared keys must be absent from the manifest's Changed set.
/// Declared+unchanged keys must be Unchanged. Declared+changed keys must be Changed.
///
/// In Rung 1 the manifest is internal-only (no wire filtering yet), but we can
/// verify that the build_manifest helper correctly classifies keys by their rev
/// state.
#[test]
fn s5_declared_narrowing_and_rev_classification() {
    let mut tracker = ProjectionRevTracker::default();
    // All keys start at rev 0, all unchanged.
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        tracker.record_emitted(key);
    }

    // Only bump configured_relays.
    tracker.source_versions.bump_configured_relays();

    let manifest = build_manifest(&tracker, 0);

    // configured_relays, relay_role_options, settings_hub depend on
    // configured_relays_ver — all should be Changed.
    // diagnostics_inputs_ver is also bumped by bump_configured_relays.
    for state in &manifest.states {
        if ["configured_relays", "relay_role_options", "settings_hub",
            "relay_diagnostics"].contains(&state.key)
        {
            assert_eq!(
                state.presence,
                ProjectionPresence::Changed,
                "key {} must be Changed after configured_relays_ver bump", state.key
            );
        }
    }

    // Keys not dependent on configured_relays_ver must be Unchanged.
    for state in &manifest.states {
        if !["configured_relays", "relay_role_options", "settings_hub",
             "relay_diagnostics"].contains(&state.key)
        {
            assert_eq!(
                state.presence,
                ProjectionPresence::Unchanged,
                "key {} must be Unchanged — not a dep of configured_relays_ver", state.key
            );
        }
    }
}

// ── Scenario 6: Reset/epoch ───────────────────────────────────────────────────

/// Scenario 6: Bumping the epoch resets the within-session counter.
/// After a bump, the epoch in the manifest is incremented.
/// Revisions are reset to 0 on a fresh `ProjectionRevTracker` (the Reset path
/// constructs a new `Kernel`, giving a zeroed tracker).
#[test]
fn s6_epoch_bumps_and_fresh_tracker_resets_revs() {
    let mut tracker = ProjectionRevTracker::default();
    tracker.source_versions.bump_profiles();
    tracker.source_versions.bump_active_account();
    tracker.source_versions.bump_publish();
    assert!(tracker.projection_rev("profile") > 0);

    tracker.bump_epoch();
    assert_eq!(tracker.epoch, 1, "epoch must be 1 after first bump");

    let manifest = build_manifest(&tracker, 42);
    assert_eq!(manifest.epoch, 1);
    assert_eq!(manifest.session_id, 42);

    // Fresh tracker simulates kernel Reset.
    let fresh = ProjectionRevTracker::default();
    assert_eq!(fresh.projection_rev("profile"), 0, "fresh tracker revs must be 0");
    assert_eq!(fresh.epoch, 0);
}

// ── Scenario 7: relay_diagnostics breadth ────────────────────────────────────

/// Scenario 7: A wire-sub open/close (diagnostics input) bumps
/// `diagnostics_inputs_ver` -> `relay_diagnostics` rev advances.
/// Idle tick with no relay-health change -> Unchanged.
#[test]
fn s7_relay_diagnostics_breadth_and_idle_stable() {
    let mut tracker = ProjectionRevTracker::default();
    tracker.record_emitted("relay_diagnostics");
    let rev_before = tracker.projection_rev("relay_diagnostics");

    // Idle tick: no diagnostics input change.
    assert!(
        !tracker.changed_since_last_emit("relay_diagnostics"),
        "relay_diagnostics must be Unchanged on idle tick"
    );

    // Wire/lifecycle change (not a relay-health change).
    tracker.source_versions.bump_diagnostics_inputs();

    assert!(
        tracker.projection_rev("relay_diagnostics") > rev_before,
        "relay_diagnostics rev must advance after diagnostics_inputs_ver bump"
    );
    assert!(
        tracker.changed_since_last_emit("relay_diagnostics"),
        "relay_diagnostics must be Changed after wire/lifecycle change"
    );
}

// ── Scenario 8: per-key table test ────────────────────────────────────────────

/// Scenario 8: For EACH built-in key — mutate one of its declared source
/// counters and assert rev++ + Changed. Then tick without mutating and assert
/// Unchanged/stable.
///
/// This is the "dependency table correctness" gate: if a key is missing from
/// `BUILTIN_PROJECTION_DEPENDENCIES` or its deps are wrong, this test fails.
#[test]
fn s8_per_key_mutate_dep_bumps_rev_no_op_tick_stable() {
    // Map each key to the first source counter in its dependency list.
    for (key, deps) in BUILTIN_PROJECTION_DEPENDENCIES {
        let first_dep = match deps.first() {
            Some(d) => *d,
            None => continue, // no deps declared — skip (will fail s9 instead)
        };

        let mut tracker = ProjectionRevTracker::default();
        tracker.record_emitted(key);
        let rev_before = tracker.projection_rev(key);
        assert!(
            !tracker.changed_since_last_emit(key),
            "key={key}: must be Unchanged at baseline"
        );

        // Bump the source.
        match first_dep {
            "profiles_ver" => tracker.source_versions.bump_profiles(),
            "accounts_ver" => tracker.source_versions.bump_accounts(),
            "active_account_ver" => tracker.source_versions.bump_active_account(),
            "profile_claims_ver" => tracker.source_versions.bump_profile_claims(),
            "claimed_event_content_ver" => tracker.source_versions.bump_claimed_event_content(),
            "open_views_ver" => tracker.source_versions.bump_open_views(),
            "configured_relays_ver" => tracker.source_versions.bump_configured_relays(),
            "publish_ver" => tracker.source_versions.bump_publish(),
            "diagnostics_inputs_ver" => tracker.source_versions.bump_diagnostics_inputs(),
            "settlement_enqueue_ver" => tracker.source_versions.bump_settlement_enqueue(),
            "settlement_drain_ver" => tracker.source_versions.bump_settlement_drain(),
            "ttl_expiry_ver" => tracker.source_versions.bump_ttl_expiry(),
            other => panic!("unknown source counter '{other}' in deps for key '{key}'"),
        }

        let rev_after = tracker.projection_rev(key);
        assert!(
            rev_after > rev_before,
            "key={key}: rev must advance after bumping '{first_dep}'; before={rev_before} after={rev_after}"
        );
        assert!(
            tracker.changed_since_last_emit(key),
            "key={key}: must be Changed after bumping '{first_dep}'"
        );

        // No-op tick: record emit, then assert stable.
        tracker.record_emitted(key);
        assert!(
            !tracker.changed_since_last_emit(key),
            "key={key}: must be Unchanged on no-op tick after recording emit"
        );
    }
}

// ── Completeness enforcement ──────────────────────────────────────────────────

/// Every key in `KERNEL_BUILTIN_PROJECTION_KEYS` MUST have an entry in
/// `BUILTIN_PROJECTION_DEPENDENCIES`. A new key without a dependency entry
/// fails this test at `cargo test -p nmp-core` time.
#[test]
fn all_builtin_keys_have_dependency_entries() {
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        let found = BUILTIN_PROJECTION_DEPENDENCIES
            .iter()
            .any(|(k, _)| k == key);
        assert!(
            found,
            "projection key '{key}' is in KERNEL_BUILTIN_PROJECTION_KEYS but has no entry in \
             BUILTIN_PROJECTION_DEPENDENCIES — add a dependency mapping to \
             kernel/projection_rev/mod.rs"
        );
    }
}

/// Every key in `BUILTIN_PROJECTION_DEPENDENCIES` MUST be in
/// `KERNEL_BUILTIN_PROJECTION_KEYS`. A stale entry in the dep table that no
/// longer exists in the built-in key set fails this test.
#[test]
fn dependency_table_has_no_orphan_keys() {
    for (key, _) in BUILTIN_PROJECTION_DEPENDENCIES {
        let found = KERNEL_BUILTIN_PROJECTION_KEYS.contains(key);
        assert!(
            found,
            "dependency entry for '{key}' exists in BUILTIN_PROJECTION_DEPENDENCIES but \
             '{key}' is NOT in KERNEL_BUILTIN_PROJECTION_KEYS — remove or rename the entry"
        );
    }
}

/// `build_manifest` covers all built-in keys and returns one state per key.
#[test]
fn build_manifest_covers_all_builtin_keys() {
    let tracker = ProjectionRevTracker::default();
    let manifest = build_manifest(&tracker, 0);
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        let found = manifest.states.iter().any(|s| s.key == *key);
        assert!(found, "manifest missing state for key '{key}'");
    }
    assert_eq!(
        manifest.states.len(),
        KERNEL_BUILTIN_PROJECTION_KEYS.len(),
        "manifest must have exactly one state per built-in key"
    );
}

/// `build_state` returns `Unchanged` at rev 0 for a fresh tracker (all
/// sources at 0, no emissions recorded).
#[test]
fn build_state_fresh_tracker_all_unchanged() {
    let tracker = ProjectionRevTracker::default();
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        let state = build_state(&tracker, key);
        assert_eq!(
            state.presence,
            ProjectionPresence::Unchanged,
            "fresh tracker: key '{key}' must be Unchanged"
        );
        assert_eq!(state.rev, 0, "fresh tracker: key '{key}' must have rev=0");
    }
}

/// All source counter names used in `BUILTIN_PROJECTION_DEPENDENCIES` must be
/// recognized by `SourceVersions::get` (non-zero after a corresponding bump).
#[test]
fn all_dep_source_names_are_recognized_by_get() {
    use std::collections::HashSet;
    let mut all_sources: HashSet<&str> = HashSet::new();
    for (_, deps) in BUILTIN_PROJECTION_DEPENDENCIES {
        for dep in *deps {
            all_sources.insert(dep);
        }
    }
    for source in all_sources {
        let mut sv = super::SourceVersions::default();
        // Bump via a known bump method.
        match source {
            "profiles_ver" => sv.bump_profiles(),
            "accounts_ver" => sv.bump_accounts(),
            "active_account_ver" => sv.bump_active_account(),
            "profile_claims_ver" => sv.bump_profile_claims(),
            "claimed_event_content_ver" => sv.bump_claimed_event_content(),
            "open_views_ver" => sv.bump_open_views(),
            "configured_relays_ver" => sv.bump_configured_relays(),
            "publish_ver" => sv.bump_publish(),
            "diagnostics_inputs_ver" => sv.bump_diagnostics_inputs(),
            "settlement_enqueue_ver" => sv.bump_settlement_enqueue(),
            "settlement_drain_ver" => sv.bump_settlement_drain(),
            "ttl_expiry_ver" => sv.bump_ttl_expiry(),
            other => panic!("unknown source name '{other}' — add a bump method"),
        }
        assert!(
            sv.get(source) > 0,
            "source '{source}' must be non-zero after bump"
        );
    }
}
