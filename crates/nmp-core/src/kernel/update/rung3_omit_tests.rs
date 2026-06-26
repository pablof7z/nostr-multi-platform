//! Unit tests for `rung3_omit::omit_unchanged` — both the forward-filter
//! (Rung 3 S1 behavior) and the inverse-pass Cleared-synthesis (S1b §10.2).
//!
//! Extracted from `rung3_omit.rs` (per §10.9 file-size pre-plan) so the
//! production transform stays under 300 LOC while test coverage expands.

use super::omit_unchanged;
use crate::kernel::projection_rev::{ProjectionManifest, ProjectionPresence, ProjectionState};
use crate::update_envelope::{TypedProjectionData, WireProjectionState};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Build a minimal `TypedProjectionData` for testing.
fn make_row(key: &str, payload: Vec<u8>) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        payload,
        state: WireProjectionState::Changed,
        projection_rev: 1,
        ..Default::default()
    }
}

/// Build a minimal `ProjectionManifest` with the given states.
fn make_manifest(states: Vec<(&'static str, ProjectionPresence, u64)>) -> ProjectionManifest {
    ProjectionManifest {
        session_id: 1,
        epoch: 0,
        states: states
            .into_iter()
            .map(|(key, presence, rev)| ProjectionState { key, presence, rev })
            .collect(),
    }
}

// ── Forward-pass (present-row filter) tests ───────────────────────────────────

/// enabled + Unchanged → row is dropped entirely.
#[test]
fn enabled_unchanged_omits_row() {
    let typed = vec![make_row("profile", vec![1, 2, 3])];
    let manifest = make_manifest(vec![("profile", ProjectionPresence::Unchanged, 0)]);
    let result = omit_unchanged(typed, &manifest, true);
    assert!(result.is_empty(), "Unchanged row must be omitted");
}

/// enabled + Cleared (present in typed) → row kept with EMPTY payload and state=Cleared.
#[test]
fn enabled_cleared_present_keeps_row_with_empty_payload() {
    let typed = vec![make_row("action_results", vec![0xde, 0xad])];
    let manifest = make_manifest(vec![("action_results", ProjectionPresence::Cleared, 2)]);
    let result = omit_unchanged(typed, &manifest, true);
    assert_eq!(result.len(), 1, "Cleared row must be kept");
    let row = &result[0];
    assert!(row.payload.is_empty(), "Cleared row payload must be empty");
    assert_eq!(
        row.state,
        WireProjectionState::Cleared,
        "Cleared row state must be Cleared"
    );
}

/// enabled + Changed → row is kept with its full payload.
#[test]
fn enabled_changed_keeps_full_row() {
    let payload = vec![1, 2, 3, 4];
    let typed = vec![make_row("accounts", payload.clone())];
    let manifest = make_manifest(vec![("accounts", ProjectionPresence::Changed, 3)]);
    let result = omit_unchanged(typed, &manifest, true);
    assert_eq!(result.len(), 1, "Changed row must be kept");
    assert_eq!(
        result[0].payload, payload,
        "Changed row payload must be unchanged"
    );
    assert_eq!(
        result[0].state,
        WireProjectionState::Changed,
        "Changed row state must be Changed"
    );
}

/// !enabled → all rows present regardless of presence.
#[test]
fn disabled_all_rows_present() {
    let typed = vec![
        make_row("profile", vec![1]),
        make_row("accounts", vec![2]),
        make_row("action_results", vec![3]),
    ];
    let manifest = make_manifest(vec![
        ("profile", ProjectionPresence::Unchanged, 0),
        ("accounts", ProjectionPresence::Cleared, 1),
        ("action_results", ProjectionPresence::Changed, 2),
    ]);
    let result = omit_unchanged(typed.clone(), &manifest, false);
    assert_eq!(
        result.len(),
        3,
        "disabled: all rows must be present regardless of presence"
    );
    // Payloads should be untouched (including the Cleared one).
    assert_eq!(
        result[1].payload,
        vec![2],
        "disabled: Cleared row payload untouched"
    );
    assert_eq!(
        result[1].state,
        WireProjectionState::Changed,
        "disabled: Cleared row state untouched (not stripped)"
    );
}

/// A key with NO manifest entry (Tier-1 host projection) is never omitted,
/// even when enabled (D3-7).
#[test]
fn tier1_no_manifest_entry_never_omitted() {
    // "nmp.feed.home" is a Tier-1 host projection — absent from manifest.
    let typed = vec![make_row("nmp.feed.home", vec![0xca, 0xfe])];
    // Manifest only covers a Tier-2 key (profile), not the feed.
    let manifest = make_manifest(vec![("profile", ProjectionPresence::Unchanged, 0)]);
    let result = omit_unchanged(typed, &manifest, true);
    assert_eq!(
        result.len(),
        1,
        "Tier-1 key absent from manifest must never be omitted"
    );
    assert_eq!(result[0].key, "nmp.feed.home");
    assert_eq!(result[0].state, WireProjectionState::Changed);
}

/// Mixed sidecar: Changed + Unchanged + Cleared + Tier-1 — validate each.
#[test]
fn mixed_sidecar_filters_correctly() {
    let typed = vec![
        make_row("profile", vec![1]),        // Changed
        make_row("accounts", vec![2]),       // Unchanged → dropped
        make_row("action_results", vec![3]), // Cleared → empty payload
        make_row("nmp.wallet", vec![4]),     // Tier-1, no manifest entry → kept
    ];
    let manifest = make_manifest(vec![
        ("profile", ProjectionPresence::Changed, 5),
        ("accounts", ProjectionPresence::Unchanged, 3),
        ("action_results", ProjectionPresence::Cleared, 6),
    ]);
    let result = omit_unchanged(typed, &manifest, true);
    // Expect: profile (Changed), action_results (Cleared+empty), nmp.wallet (Tier-1).
    // accounts (Unchanged) must be dropped.
    assert_eq!(result.len(), 3);
    let profile = result
        .iter()
        .find(|r| r.key == "profile")
        .expect("profile present");
    assert_eq!(profile.state, WireProjectionState::Changed);
    assert_eq!(profile.payload, vec![1]);

    let ar = result
        .iter()
        .find(|r| r.key == "action_results")
        .expect("action_results present");
    assert!(ar.payload.is_empty(), "Cleared row must have empty payload");
    assert_eq!(ar.state, WireProjectionState::Cleared);

    let wallet = result
        .iter()
        .find(|r| r.key == "nmp.wallet")
        .expect("nmp.wallet present");
    assert_eq!(wallet.payload, vec![4]);
    assert_eq!(wallet.state, WireProjectionState::Changed);

    assert!(
        result.iter().all(|r| r.key != "accounts"),
        "accounts (Unchanged) must be absent"
    );
}

/// Empty typed sidecar with enabled omission — result is also empty.
#[test]
fn empty_typed_sidecar_stays_empty() {
    let manifest = make_manifest(vec![("profile", ProjectionPresence::Unchanged, 0)]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert!(result.is_empty());
}

/// Empty manifest with enabled omission — all rows treated as Tier-1 (Changed).
#[test]
fn empty_manifest_treats_all_as_tier1() {
    let typed = vec![make_row("custom.key", vec![99])];
    let manifest = make_manifest(vec![]);
    let result = omit_unchanged(typed, &manifest, true);
    assert_eq!(
        result.len(),
        1,
        "no manifest entry → Tier-1 default (Changed) → kept"
    );
    assert_eq!(result[0].key, "custom.key");
}

// ── Inverse-pass (Cleared-synthesis) tests — §10.2 / issue #1390 ─────────────

/// manifest-Cleared + key ABSENT from typed → synthesized Cleared row.
/// This is the fix for drain keys (action_results, signed_events) on the
/// non-empty→empty transition.
#[test]
fn manifest_cleared_absent_synthesizes_cleared_row() {
    // No typed row for action_results, but the manifest says Cleared.
    let manifest = make_manifest(vec![
        ("action_results", ProjectionPresence::Cleared, 7),
        ("profile", ProjectionPresence::Unchanged, 3),
    ]);
    let result = omit_unchanged(vec![], &manifest, true);
    // Only action_results (Cleared) should be synthesized.
    // profile is Unchanged-absent → nothing.
    assert_eq!(result.len(), 1, "synthesized Cleared row must appear");
    let row = &result[0];
    assert_eq!(row.key, "action_results");
    assert_eq!(row.state, WireProjectionState::Cleared);
    assert!(
        row.payload.is_empty(),
        "synthesized Cleared row must have empty payload"
    );
    assert_eq!(
        row.projection_rev, 7,
        "synthesized row must carry manifest rev"
    );
}

/// manifest-Cleared for signed_events (second drain key) — synthesized.
#[test]
fn signed_events_manifest_cleared_absent_synthesizes() {
    let manifest = make_manifest(vec![("signed_events", ProjectionPresence::Cleared, 5)]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].key, "signed_events");
    assert_eq!(result[0].state, WireProjectionState::Cleared);
}

/// manifest-Cleared for action_stages — synthesized (post §10.4 edge machine).
#[test]
fn action_stages_manifest_cleared_absent_synthesizes() {
    let manifest = make_manifest(vec![("action_stages", ProjectionPresence::Cleared, 9)]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].key, "action_stages");
    assert_eq!(result[0].state, WireProjectionState::Cleared);
    assert!(result[0].payload.is_empty());
}

/// manifest-Cleared for action_lifecycle — synthesized.
#[test]
fn action_lifecycle_manifest_cleared_absent_synthesizes() {
    let manifest = make_manifest(vec![("action_lifecycle", ProjectionPresence::Cleared, 11)]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].key, "action_lifecycle");
    assert_eq!(result[0].state, WireProjectionState::Cleared);
}

/// manifest-Changed + CONDITIONAL key + absent → defensive belt synthesizes Cleared.
/// (§10.2 belt case: Changed-but-absent on a conditional key unambiguously
/// means "went empty".)
#[test]
fn conditional_key_changed_absent_belt_synthesizes_cleared() {
    let manifest = make_manifest(vec![("action_results", ProjectionPresence::Changed, 8)]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert_eq!(
        result.len(),
        1,
        "belt must synthesize Cleared for conditional Changed-absent"
    );
    assert_eq!(result[0].key, "action_results");
    assert_eq!(result[0].state, WireProjectionState::Cleared);
}

/// manifest-Changed + NON-CONDITIONAL (unconditional) key + absent
/// → invariant violation: in debug builds the `debug_assert!` fires (panics),
/// signaling a producer bug. The producer bug is: an unconditional Tier-2 key
/// reported as `Changed` in the manifest but emitted no typed row.
///
/// This test verifies the debug-mode invariant check fires, which is the
/// correct behavior (a Changed-but-absent unconditional key is a BUG that
/// MUST be surfaced loudly — synthesizing Cleared here would delete live host
/// state and mask the underlying bug).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "rung3_omit invariant violation")]
fn unconditional_key_changed_absent_panics_in_debug() {
    // "profile" is an unconditional Tier-2 key — it MUST always emit a row
    // when Changed. A Changed-but-absent profile is a producer bug.
    let manifest = make_manifest(vec![("profile", ProjectionPresence::Changed, 5)]);
    // This must panic with the invariant-violation debug_assert.
    let _result = omit_unchanged(vec![], &manifest, true);
}

/// A present Changed key must NOT be shadowed by the inverse pass.
/// One row per key per frame (§10.3 codex #2).
#[test]
fn present_key_not_double_emitted() {
    let typed = vec![make_row("action_results", vec![0xAA, 0xBB])];
    // manifest says Cleared for action_results, but it IS present in typed.
    // The forward pass strips its payload (Cleared + present); the inverse
    // pass must NOT also synthesize a second row.
    let manifest = make_manifest(vec![("action_results", ProjectionPresence::Cleared, 3)]);
    let result = omit_unchanged(typed, &manifest, true);
    let count = result.iter().filter(|r| r.key == "action_results").count();
    assert_eq!(count, 1, "exactly one Cleared row, not two");
    assert!(result[0].payload.is_empty());
    assert_eq!(result[0].state, WireProjectionState::Cleared);
}

/// manifest-Unchanged + absent → no synthesis (stably-empty; correct).
#[test]
fn unchanged_absent_never_synthesized() {
    let manifest = make_manifest(vec![
        ("action_results", ProjectionPresence::Unchanged, 0),
        ("action_stages", ProjectionPresence::Unchanged, 0),
    ]);
    let result = omit_unchanged(vec![], &manifest, true);
    assert!(
        result.is_empty(),
        "Unchanged absent keys must never produce Cleared rows"
    );
}

/// When disabled (no incremental-apply), the inverse pass must NOT run.
#[test]
fn disabled_inverse_pass_does_not_run() {
    let manifest = make_manifest(vec![("action_results", ProjectionPresence::Cleared, 5)]);
    // disabled: return typed unchanged — no synthesis
    let result = omit_unchanged(vec![], &manifest, false);
    assert!(
        result.is_empty(),
        "disabled: inverse pass must not synthesize"
    );
}
