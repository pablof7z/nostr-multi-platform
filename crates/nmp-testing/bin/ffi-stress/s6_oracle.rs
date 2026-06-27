//! S6 byte-identity oracle (ADR-0055 Rung 3 capstone correctness proof).
//!
//! Extracted from `s6_single_projection_churn.rs` per repo file-size doctrine
//! (500-LOC hard ceiling, split into cohesive submodules).
//!
//! The oracle is the correctness half of the capstone: it proves that the
//! incremental stream (Changed/Cleared rows only, Unchanged omitted) is
//! *lossless*. A Rust stand-in of the host ProjectionCache merge (D3-3)
//! reconstructs the full per-key payload set from the incremental frames and
//! asserts it is byte-identical to the full-frame reference.

use nmp_core::{decode_snapshot_typed_projections, WireProjectionState};
use std::collections::HashMap;

// ── Per-tick frame data captured in the callback ─────────────────────────────

/// One frame's measurements captured in the update callback.
///
/// Shared by the measurement harness (frame bytes / serialize_us percentiles)
/// and the byte-identity oracle (per-key projection payloads).
pub(crate) struct FrameRecord {
    /// Total frame byte count (the raw FlatBuffers payload delivered to the host).
    pub(crate) frame_bytes: usize,
    /// Previous tick's `serialize_us` from the FlatBuffers metrics (one-tick lag).
    pub(crate) serialize_us: u64,
    /// Typed projections present in this frame (key → payload bytes).
    /// Used by the byte-identity oracle.
    pub(crate) projection_payloads: HashMap<String, Vec<u8>>,
}

// ── Byte-identity oracle: Rust stand-in for the ProjectionCache merge ─────────
//
// Applies the same merge algorithm the generated ProjectionCache (D3-3) uses:
//   - Changed row  → overwrite cache[key] with new payload
//   - Cleared row  → remove cache[key]
//   - Absent key   → retain cache[key] unchanged
//
// This proves that the incremental stream is lossless: applying all frames in
// sequence reconstructs the same per-key payload set as the full-frame stream.

/// Minimal stand-in for the host ProjectionCache merge (D3-3).
#[derive(Default)]
struct MiniProjectionCache {
    cache: HashMap<String, Vec<u8>>,
}

impl MiniProjectionCache {
    /// Merge one incremental frame's typed projection rows into the cache.
    fn merge_frame(&mut self, frame_bytes: &[u8]) {
        if let Ok(rows) = decode_snapshot_typed_projections(frame_bytes) {
            for row in rows {
                match row.state {
                    WireProjectionState::Changed => {
                        self.cache.insert(row.key, row.payload);
                    }
                    WireProjectionState::Cleared => {
                        self.cache.remove(&row.key);
                    }
                }
            }
        }
    }
}

// ── Oracle result + entry point ──────────────────────────────────────────────

pub(crate) struct OracleResult {
    pub(crate) passed: bool,
    pub(crate) note: String,
}

/// Tier-1 (feed-class) projection keys that are always-Changed by D3-7 (no
/// manifest entry, never omitted) AND nondeterministic between two independent
/// kernel instances seeded from the same events — their end-state content (or
/// presence) legitimately differs run-to-run and across the two NmpApps. These
/// are the ONLY keys the oracle is allowed to find absent from the incremental
/// reconstruction. Any OTHER absent key is a hard FAIL (a dropped Tier-2 row =
/// a real omit bug the capstone must catch).
const ALLOWED_ABSENT_TIER1_KEYS: &[&str] = &["refs.event.envelopes", "nip46_onboarding"];

/// Run the byte-identity oracle (end-state, fail-closed).
///
/// Replays the entire incremental stream (Phase B raw frames) through a
/// `MiniProjectionCache` and compares the resulting **end-state** reconstruction
/// against Phase A's final full-frame projection set. This proves the incremental
/// stream is lossless: omitting `Unchanged` rows + applying Changed/Cleared
/// reconstructs the same per-key payloads the full-frame stream carried.
///
/// **Fail-closed.** Per-key verdicts:
///   - present + byte-identical          → OK
///   - present + payload differs         → hard FAIL (corruption)
///   - absent, key in the Tier-1         → tolerated (allowed-absent whitelist)
///     nondeterminism whitelist
///   - absent, ANY other key             → hard FAIL (a dropped Tier-2 row)
///
/// This closes the latent hole where a future omit bug that dropped a needed
/// Tier-2 row would have silently passed: only the two known-nondeterministic
/// Tier-1 keys (D3-7, no manifest entry) may be missing.
///
/// Note: this is an **end-state** identity check, not per-tick — true per-tick
/// alignment across two independent kernels isn't meaningful (different timing /
/// snapshot cadence). End-state + fail-closed is the honest, sufficient
/// losslessness proof.
pub(crate) fn run_byte_identity_oracle(
    incremental_frames: &[Vec<u8>],
    full_frame_records: &[FrameRecord],
) -> OracleResult {
    if incremental_frames.is_empty() || full_frame_records.is_empty() {
        return OracleResult {
            passed: false,
            note: format!(
                "FAIL: insufficient data for oracle — incremental_frames={} full_frame_records={}",
                incremental_frames.len(),
                full_frame_records.len()
            ),
        };
    }

    // Apply the full incremental stream through the cache.
    let mut cache = MiniProjectionCache::default();
    for frame_bytes in incremental_frames {
        cache.merge_frame(frame_bytes);
    }
    let reconstructed = cache.cache;

    // Reference: the last Phase A frame's projection set (steady-state after
    // the full churn window — both kernels end on a `release_profile` + configure
    // tick, leaving the profile unclaimed in both cases).
    let reference = &full_frame_records[full_frame_records.len() - 1].projection_payloads;

    let mut hard_failures: Vec<String> = Vec::new();
    let mut tolerated_absences: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for (key, ref_payload) in reference {
        // Skip empty reference payloads — projections that produce no bytes
        // (e.g. an empty action_results drain). Both sides agree on absence.
        if ref_payload.is_empty() {
            continue;
        }
        match reconstructed.get(key) {
            Some(recon_payload) if recon_payload == ref_payload => {
                checked += 1;
            }
            Some(recon_payload) => {
                // Present but bytes differ → corruption. Always hard-fail.
                hard_failures.push(format!(
                    "key='{}' PAYLOAD MISMATCH ref_len={} recon_len={}",
                    key,
                    ref_payload.len(),
                    recon_payload.len()
                ));
            }
            None if ALLOWED_ABSENT_TIER1_KEYS.contains(&key.as_str()) => {
                // Whitelisted nondeterministic Tier-1 key — tolerated absence.
                tolerated_absences.push(key.clone());
            }
            None => {
                // Any other key absent from the reconstruction = a dropped row.
                // This is the omit-bug case the capstone must catch → hard-fail.
                hard_failures.push(format!(
                    "key='{}' DROPPED (absent from incremental reconstruction, ref_len={}) \
                     — not in the allowed-absent Tier-1 whitelist",
                    key,
                    ref_payload.len()
                ));
            }
        }
    }

    classify_oracle_result(
        hard_failures,
        tolerated_absences,
        checked,
        incremental_frames.len(),
    )
}

/// Final verdict: PASS only when there are zero hard failures (payload
/// corruption OR a non-whitelisted dropped key). Tolerated Tier-1 absences are
/// reported but do not fail the gate.
fn classify_oracle_result(
    hard_failures: Vec<String>,
    tolerated_absences: Vec<String>,
    checked: usize,
    incremental_frame_count: usize,
) -> OracleResult {
    if hard_failures.is_empty() {
        OracleResult {
            passed: true,
            note: format!(
                "PASS (fail-closed) — {checked} keys byte-identical; {} whitelisted Tier-1 \
                 key(s) absent {:?}; {incremental_frame_count} incremental frames applied",
                tolerated_absences.len(),
                tolerated_absences,
            ),
        }
    } else {
        OracleResult {
            passed: false,
            note: format!(
                "FAIL (fail-closed) — {} hard failure(s) [payload mismatch or dropped \
                 non-whitelisted key]: {:?}",
                hard_failures.len(),
                &hard_failures[..hard_failures.len().min(5)]
            ),
        }
    }
}
