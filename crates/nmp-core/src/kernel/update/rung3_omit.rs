//! ADR-0070 Rung 3 S1b — omit `Unchanged` projections from the wire frame
//! and synthesize `Cleared` rows for conditionally-present keys whose tracker
//! went empty this tick.
//!
//! Pure transform: given the typed sidecar built this tick (post-Rung-2
//! stamping) and the per-tick manifest, drop rows whose presence is
//! `Unchanged` when omission is enabled, strip payloads from `Cleared` rows
//! in the present set, synthesize explicit `Cleared` rows for manifest-Cleared
//! keys absent from `typed`, and keep `Changed` rows intact.
//!
//! Mirrors `rung2_stamp.rs` in structure: a single pure function with no
//! side-effects on kernel state.
//!
//! ## Invariants (ADR-0070 §3 D3-1 / D3-2 / D3-7 + §10.2 / §10.3)
//!
//! - `!enabled` → return `typed` unchanged (full rows, no omission).
//! - `enabled` + `Unchanged` → DROP the row entirely (absence == Unchanged
//!   on the wire; not an empty marker row — D3-1).
//! - `enabled` + `Cleared` (present in typed) → keep the row with EMPTY
//!   payload and `state = Cleared` so the host cache can drop its prior
//!   value (D3-1).
//! - `enabled` + `Changed` → keep the full row.
//! - **Inverse pass (§10.2):** for manifest entries NOT in `typed`:
//!   - `Cleared`  → always synthesize a payload-less `Cleared` row.
//!   - `Changed` && key ∈ `CONDITIONAL_PRESENCE_KEYS` → synthesize `Cleared`
//!     (defensive belt: for these four keys absence == went-empty).
//!   - `Changed` && key ∉ `CONDITIONAL_PRESENCE_KEYS` → invariant violation;
//!     `debug_assert!` + `warn!`; do NOT synthesize (preserve sharpness).
//!   - `Unchanged` (absent from typed) → no action (correct — stably empty).
//! - A key with NO manifest entry (Tier-1 host projections) defaults to
//!   `Changed` — always kept, never omitted (D3-7).

use crate::kernel::projection_rev::{
    ProjectionManifest, ProjectionPresence, CONDITIONAL_PRESENCE_KEYS,
};
use crate::update_envelope::{TypedProjectionData, WireProjectionState};

/// Apply the Rung-3 omission transform with Cleared-synthesis.
///
/// `typed`: the stamped typed sidecar (post `rung2_stamp::stamp_typed_projections`).
/// `manifest`: the per-tick manifest whose `states` carry the `presence` field.
/// `enabled`: whether the host has declared incremental-apply capability.
///
/// When `!enabled`, returns `typed` unchanged — the kernel emits full rows for
/// every non-advertising host (no behavior change from Rung 2). When `enabled`,
/// rows are filtered / stripped per the presence rules above, AND the inverse
/// pass synthesizes `Cleared` rows for conditionally-present keys absent from
/// the sidecar (§10.2 fix for issue #1390).
#[must_use]
pub(super) fn omit_unchanged(
    typed: Vec<TypedProjectionData>,
    manifest: &ProjectionManifest,
    enabled: bool,
) -> Vec<TypedProjectionData> {
    if !enabled {
        return typed;
    }

    // ── Forward pass: filter rows already present in `typed` ─────────────────
    let mut out: Vec<TypedProjectionData> = typed
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
        .collect();

    // ── Inverse pass: synthesize Cleared rows for absent manifest entries ─────
    // Collect the keys already emitted so we can skip them in the inverse pass.
    // One Cleared row per key per frame — never shadow a Changed row (§10.3
    // codex #2 requirement: "one row per key per frame").
    // Use owned Strings so the HashSet does NOT borrow `out`, allowing the
    // subsequent `out.push(...)` calls to compile without a borrow conflict.
    let present_keys: std::collections::HashSet<String> =
        out.iter().map(|e| e.key.clone()).collect();

    for ps in &manifest.states {
        if present_keys.contains(ps.key) {
            continue; // already in `out`; do not double-emit
        }
        match ps.presence {
            // manifest-Cleared AND absent from typed → synthesize a payload-less
            // Cleared row. This is the primary fix for drain keys (action_results,
            // signed_events) and — after §10.4 `note_copy_emit` — also fires for
            // action_stages / action_lifecycle on their Cleared edge.
            ProjectionPresence::Cleared => {
                out.push(TypedProjectionData {
                    key: ps.key.to_string(),
                    state: WireProjectionState::Cleared,
                    projection_rev: ps.rev,
                    ..Default::default() // empty payload, schema_id/version/file_id
                });
            }
            // manifest-Changed AND key ∈ CONDITIONAL_PRESENCE_KEYS AND absent
            // → synthesize Cleared (defensive belt, §10.2).
            // For these four keys the accessor returns Null iff empty, so a
            // Changed-but-absent entry ALWAYS means "went empty" and the only
            // safe signal is Cleared.
            ProjectionPresence::Changed if CONDITIONAL_PRESENCE_KEYS.contains(&ps.key) => {
                out.push(TypedProjectionData {
                    key: ps.key.to_string(),
                    state: WireProjectionState::Cleared,
                    projection_rev: ps.rev,
                    ..Default::default()
                });
            }
            // manifest-Changed AND key ∉ CONDITIONAL_PRESENCE_KEYS AND absent
            // → invariant violation: an unconditional Tier-2 key must always
            // produce a row when Changed. Do NOT synthesize (that would silently
            // delete live host state and mask the producer bug). Signal loudly.
            ProjectionPresence::Changed => {
                debug_assert!(
                    false,
                    "rung3_omit invariant violation: manifest-Changed key '{}' is absent \
                     from typed — this is a producer bug (an unconditional Tier-2 key \
                     must always emit a row when Changed). Do NOT synthesize Cleared here \
                     (would delete live host state). File a bug and investigate. \
                     (ADR-0070 §10.2 / issue #1390)",
                    ps.key
                );
                tracing::warn!(
                    key = ps.key,
                    rev = ps.rev,
                    "rung3_omit: manifest-Changed Tier-2 key absent from typed sidecar \
                     — producer invariant violated; row skipped to preserve host state"
                );
            }
            // manifest-Unchanged AND absent → correct; stably-empty key.
            // No action needed (absence == Unchanged on the wire).
            ProjectionPresence::Unchanged => {}
        }
    }

    out
}

#[cfg(test)]
#[path = "rung3_omit_tests.rs"]
mod tests;
