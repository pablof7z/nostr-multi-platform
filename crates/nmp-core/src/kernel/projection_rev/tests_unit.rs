//! ADR-0055 Rung 1 — dependency-table completeness + arithmetic unit tests.
//!
//! Split out of `tests.rs` (which holds the REAL-driven scenario tests) so each
//! test file stays under the 500-LOC hard ceiling (AGENTS.md). These tests
//! exercise the tracker arithmetic and the co-location contract directly — they
//! do NOT drive a real kernel (that is `tests.rs`'s job).

use crate::kernel::projection_rev::{
    build_manifest, build_state, ProjectionPresence, ProjectionRevTracker, DRAIN_PROJECTION_KEYS,
    BUILTIN_PROJECTION_DEPENDENCIES,
};
use crate::kernel::update::KERNEL_BUILTIN_PROJECTION_KEYS;

/// Bump one named source counter on `tracker`. Shared by the table tests.
fn bump_named(tracker: &mut ProjectionRevTracker, source: &str, key: &str) {
    match source {
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
}

// ── Per-key arithmetic table (tracker unit test) ──────────────────────────────

/// For EACH built-in key, mutate one declared source counter and assert
/// rev++ + Changed, then a no-op tick is Unchanged. This is a focused unit test
/// of the dependency table + SUM arithmetic (NOT the real kernel path — that is
/// what `tests.rs` S1–S9 cover). Skips the drain keys, whose presence is driven
/// by the `note_drain_emit` state machine rather than a bare source bump.
#[test]
fn s8_per_key_mutate_dep_bumps_rev_no_op_tick_stable() {
    for (key, deps) in BUILTIN_PROJECTION_DEPENDENCIES {
        if DRAIN_PROJECTION_KEYS.contains(key) {
            continue; // presence is tristate-driven; covered by S3.
        }
        let Some(first_dep) = deps.first().copied() else {
            continue;
        };

        let mut tracker = ProjectionRevTracker::default();
        tracker.record_emitted(key);
        let rev_before = tracker.projection_rev(key);
        assert!(
            !tracker.changed_since_last_emit(key),
            "key={key}: must be Unchanged at baseline"
        );

        bump_named(&mut tracker, first_dep, key);

        let rev_after = tracker.projection_rev(key);
        assert!(
            rev_after > rev_before,
            "key={key}: rev must advance after bumping '{first_dep}'; \
             before={rev_before} after={rev_after}"
        );
        assert!(
            tracker.changed_since_last_emit(key),
            "key={key}: must be Changed after bumping '{first_dep}'"
        );

        tracker.record_emitted(key);
        assert!(
            !tracker.changed_since_last_emit(key),
            "key={key}: must be Unchanged on no-op tick after recording emit"
        );
    }
}

// ── Completeness enforcement ──────────────────────────────────────────────────

/// Every key in `KERNEL_BUILTIN_PROJECTION_KEYS` MUST have an entry in
/// `BUILTIN_PROJECTION_DEPENDENCIES`.
#[test]
fn all_builtin_keys_have_dependency_entries() {
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        let found = BUILTIN_PROJECTION_DEPENDENCIES.iter().any(|(k, _)| k == key);
        assert!(
            found,
            "projection key '{key}' is in KERNEL_BUILTIN_PROJECTION_KEYS but has no entry in \
             BUILTIN_PROJECTION_DEPENDENCIES — add a dependency mapping to \
             kernel/projection_rev/mod.rs"
        );
    }
}

/// Every key in `BUILTIN_PROJECTION_DEPENDENCIES` MUST be in
/// `KERNEL_BUILTIN_PROJECTION_KEYS`.
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

/// `build_manifest` covers all built-in keys, one state per key.
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

/// `build_state` returns `Changed` at rev 0 for a fresh tracker.
///
/// ADR-0055 Rung 3 (D3-5): a key absent from `last_emitted` (never emitted,
/// or cleared by `reset_last_emitted` / `bump_epoch`) is treated as `Changed`
/// regardless of the current rev. This ensures that a full-baseline frame is
/// emitted the very first time — even when all source versions are still 0.
/// `Unchanged` requires a PRIOR emit at the same rev; absence == never-emitted
/// == Changed.
#[test]
fn build_state_fresh_tracker_all_changed() {
    let tracker = ProjectionRevTracker::default();
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        let s = build_state(&tracker, key);
        // Drain keys use the `pending_presence` / `note_drain_emit` state
        // machine rather than `last_emitted`; their default presence when
        // `pending_presence` is empty is also driven by `changed_since_last_emit`
        // which returns `true` (None → Changed), so drain keys are Changed too
        // on a completely fresh tracker.
        assert_eq!(
            s.presence,
            ProjectionPresence::Changed,
            "fresh tracker: key '{key}' must be Changed (never emitted)"
        );
        assert_eq!(s.rev, 0, "fresh tracker: key '{key}' must have rev=0");
    }
}

/// `build_state` returns `Unchanged` at rev 0 after `record_emitted`.
///
/// After emitting a key at rev 0 and recording it, the next tick with no
/// source mutations returns `Unchanged`.
#[test]
fn build_state_after_emit_at_rev0_is_unchanged() {
    let mut tracker = ProjectionRevTracker::default();
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        // Skip drain keys: their presence is driven by `note_drain_emit`, not
        // by `record_emitted` + rev comparison.
        if DRAIN_PROJECTION_KEYS.contains(key) {
            continue;
        }
        tracker.record_emitted(key);
    }
    for key in KERNEL_BUILTIN_PROJECTION_KEYS {
        if DRAIN_PROJECTION_KEYS.contains(key) {
            continue;
        }
        let s = build_state(&tracker, key);
        assert_eq!(
            s.presence,
            ProjectionPresence::Unchanged,
            "after record_emitted at rev=0: key '{key}' must be Unchanged"
        );
        assert_eq!(s.rev, 0, "after record_emitted at rev=0: key '{key}' must have rev=0");
    }
}

/// All source counter names used in `BUILTIN_PROJECTION_DEPENDENCIES` must be
/// recognized by `SourceVersions::get`.
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
        let mut tracker = ProjectionRevTracker::default();
        bump_named(&mut tracker, source, "<probe>");
        assert!(
            tracker.source_versions.get(source) > 0,
            "source '{source}' must be non-zero after bump"
        );
    }
}

// ── Oracle bite (F1 regression catcher) ───────────────────────────────────────

/// S1-bite: prove the oracle would CATCH the F1 regression. We simulate the old
/// (buggy) behaviour by claiming a longform coord and ingesting the article
/// WITHOUT the chokepoint having bumped — achieved by reaching past the public
/// path: we manually clear the bump effect, then assert make_update panics.
///
/// Rather than fork production code, we reproduce the *shape* of the bug at the
/// tracker level and confirm the oracle's StaleStamp direction fires: a payload
/// that changes while presence stays Unchanged is a violation.
#[test]
fn s1_bite_oracle_catches_stale_claimed_events() {
    use crate::kernel::projection_rev::oracle::{check_oracle, OracleViolationKind};
    use crate::update_envelope::TypedProjectionData;

    fn typed(key: &str, payload: &[u8]) -> TypedProjectionData {
        TypedProjectionData {
            key: key.to_string(),
            schema_id: String::new(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: payload.to_vec(),
            ..Default::default()
        }
    }

    let mut tracker = ProjectionRevTracker::default();
    tracker.record_emitted("claimed_events");
    let manifest_prev = build_manifest(&tracker, 0);

    // Previous emit: claimed_events carried payload P0.
    let typed_prev = vec![typed("claimed_events", b"P0")];
    // Seed prev fingerprints exactly as record_tick would.
    let prev_fps = {
        let mut oracle = crate::kernel::projection_rev::oracle::OracleState::default();
        oracle.record_tick(&manifest_prev, &typed_prev, &mut tracker);
        oracle.prev_fingerprints
    };

    // This tick: payload CHANGED to P1 but NO source-version bump happened
    // (the F1 bug) -> presence is Unchanged. The oracle must flag StaleStamp.
    let manifest_now = build_manifest(&tracker, 0);
    let typed_now = vec![typed("claimed_events", b"P1")];
    let violations = check_oracle(&prev_fps, &manifest_now, &typed_now);
    assert!(
        violations
            .iter()
            .any(|v| v.key == "claimed_events" && v.kind == OracleViolationKind::StaleStamp),
        "oracle MUST flag StaleStamp when claimed_events payload changes but rev \
         does not advance (the F1 regression); got {violations:?}"
    );
}

