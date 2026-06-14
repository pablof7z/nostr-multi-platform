//! ADR-0055 Rung 3 — omit `Unchanged` projections from the wire frame.
//!
//! Pure transform: given the typed sidecar built this tick (post-Rung-2
//! stamping) and the per-tick manifest, drop rows whose presence is
//! `Unchanged` when omission is enabled, strip payloads from `Cleared` rows,
//! and keep `Changed` rows intact.
//!
//! Mirrors `rung2_stamp.rs` in structure: a single pure function with no
//! side-effects on kernel state.
//!
//! ## Invariants (ADR-0055 §3 D3-1 / D3-2 / D3-7)
//!
//! - `!enabled` → return `typed` unchanged (full rows, no omission).
//! - `enabled` + `Unchanged` → DROP the row entirely (absence == Unchanged
//!   on the wire; not an empty marker row — D3-1).
//! - `enabled` + `Cleared` → keep the row with EMPTY payload and
//!   `state = Cleared` (explicit drop signal for the host cache — D3-1).
//! - `enabled` + `Changed` → keep the full row.
//! - A key with NO manifest entry (Tier-1 host projections) defaults to
//!   `Changed` — always kept, never omitted (D3-7: Tier-1 projections are
//!   always-overwrite in Rung 3).

use crate::kernel::projection_rev::{ProjectionManifest, ProjectionPresence};
use crate::update_envelope::{TypedProjectionData, WireProjectionState};

/// Apply the Rung-3 omission transform.
///
/// `typed`: the stamped typed sidecar (post `rung2_stamp::stamp_typed_projections`).
/// `manifest`: the per-tick manifest whose `states` carry the `presence` field.
/// `enabled`: whether the host has declared incremental-apply capability.
///
/// When `!enabled`, returns `typed` unchanged — the kernel emits full rows for
/// every non-advertising host (no behavior change from Rung 2). When `enabled`,
/// rows are filtered / stripped per the presence rules above.
#[must_use]
pub(super) fn omit_unchanged(
    typed: Vec<TypedProjectionData>,
    manifest: &ProjectionManifest,
    enabled: bool,
) -> Vec<TypedProjectionData> {
    if !enabled {
        return typed;
    }
    typed
        .into_iter()
        .filter_map(|mut entry| {
            // Look up this key's presence in the manifest.
            // If the key is NOT in the manifest (Tier-1 host projection), it
            // defaults to Changed — always kept, never omitted (D3-7).
            let presence = manifest
                .states
                .iter()
                .find(|s| s.key == entry.key.as_str())
                .map(|s| s.presence)
                .unwrap_or(ProjectionPresence::Changed);

            match presence {
                // Changed: keep the full row as-is.
                ProjectionPresence::Changed => Some(entry),
                // Cleared: keep the row but with EMPTY payload and state=Cleared
                // so the host cache can drop its prior value (D3-1).
                ProjectionPresence::Cleared => {
                    entry.payload = Vec::new();
                    entry.state = WireProjectionState::Cleared;
                    Some(entry)
                }
                // Unchanged: DROP the row entirely.
                // Absence == Unchanged on the wire (D3-1).
                ProjectionPresence::Unchanged => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::projection_rev::{ProjectionManifest, ProjectionPresence, ProjectionState};
    use crate::update_envelope::{TypedProjectionData, WireProjectionState};

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

    // ── Core omission cases ──────────────────────────────────────────────────

    /// enabled + Unchanged → row is dropped entirely.
    #[test]
    fn enabled_unchanged_omits_row() {
        let typed = vec![make_row("profile", vec![1, 2, 3])];
        let manifest = make_manifest(vec![("profile", ProjectionPresence::Unchanged, 0)]);
        let result = omit_unchanged(typed, &manifest, true);
        assert!(result.is_empty(), "Unchanged row must be omitted");
    }

    /// enabled + Cleared → row is kept with EMPTY payload and state=Cleared.
    #[test]
    fn enabled_cleared_keeps_row_with_empty_payload() {
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
        assert_eq!(result[0].payload, payload, "Changed row payload must be unchanged");
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
        assert_eq!(result[1].payload, vec![2], "disabled: Cleared row payload untouched");
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
        assert_eq!(result.len(), 1, "Tier-1 key absent from manifest must never be omitted");
        assert_eq!(result[0].key, "nmp.feed.home");
        assert_eq!(result[0].state, WireProjectionState::Changed);
    }

    /// Mixed sidecar: Changed + Unchanged + Cleared + Tier-1 — validate each.
    #[test]
    fn mixed_sidecar_filters_correctly() {
        let typed = vec![
            make_row("profile", vec![1]),        // Changed
            make_row("accounts", vec![2]),        // Unchanged → dropped
            make_row("action_results", vec![3]),  // Cleared → empty payload
            make_row("nmp.wallet", vec![4]),      // Tier-1, no manifest entry → kept
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
        let profile = result.iter().find(|r| r.key == "profile").expect("profile present");
        assert_eq!(profile.state, WireProjectionState::Changed);
        assert_eq!(profile.payload, vec![1]);

        let ar = result.iter().find(|r| r.key == "action_results").expect("action_results present");
        assert!(ar.payload.is_empty(), "Cleared row must have empty payload");
        assert_eq!(ar.state, WireProjectionState::Cleared);

        let wallet = result.iter().find(|r| r.key == "nmp.wallet").expect("nmp.wallet present");
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
        assert_eq!(result.len(), 1, "no manifest entry → Tier-1 default (Changed) → kept");
        assert_eq!(result[0].key, "custom.key");
    }
}
