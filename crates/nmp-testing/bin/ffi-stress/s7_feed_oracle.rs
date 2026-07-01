//! S7 byte-identity oracle for the feed-idle capstone (ADR-0055 R6-S4).
//!
//! Extracted per repo file-size doctrine (500-LOC hard ceiling). Mirrors the
//! structure of [`crate::s6_oracle`] but updated for the R6-S4 scenario:
//!
//! - The oracle proves LOSSLESSNESS: replaying the Phase B incremental stream
//!   through `MiniProjectionCache` reconstructs the same end-state as the Phase A
//!   full-frame reference. `"nmp.testing.feed_idle"` is present in the reconstruction from
//!   the first Phase B tick (the full baseline after `declare_incremental_apply`)
//!   and stays there, retained from cache, on subsequent idle ticks where it is
//!   OMITTED (Unchanged → byte-equality gate fires → host retains prior value).
//!   Because the feed must be present, it is deliberately NOT in `ALLOWED_ABSENT`:
//!   a missing feed key is a hard FAIL.
//!
//! - `"refs.event.envelopes"` and `"nip46_onboarding"` are the only whitelisted
//!   absences (nondeterministic Tier-1 keys, always-Changed by D3-7; no manifest
//!   entry; legitimately differ between the two independent kernel instances).
//!
//! - The `MiniProjectionCache` models ONLY the steady-state Changed/Cleared/retain
//!   subset of the host merge — NOT the session/epoch rebaseline path (this
//!   scenario never bumps session/epoch; that path is proven by R6-S1's tests).
//!   See the cache section doc.
//!
//! Fail-closed: a dropped key that is NOT in the whitelist is a hard FAIL.

use nmp_core::{decode_snapshot_typed_projections, WireProjectionState};
use std::collections::HashMap;

// ── Per-tick frame record ─────────────────────────────────────────────────────

/// One frame's measurements captured in the update callback.
pub(crate) struct FeedFrameRecord {
    /// Total frame byte count.
    pub(crate) frame_bytes: usize,
    /// Previous tick's `serialize_us` (one-tick lag).
    pub(crate) serialize_us: u64,
    /// Typed projections present in this frame (key → payload bytes).
    pub(crate) projection_payloads: HashMap<String, Vec<u8>>,
    /// Whether `nmp.testing.feed_idle` was present (Changed) in this frame.
    pub(crate) feed_present: bool,
    /// Byte count of `nmp.testing.feed_idle` in this frame (0 when absent).
    pub(crate) feed_bytes: usize,
}

// ── MiniProjectionCache (steady-state Changed/Cleared/retain subset) ─────────
//
// NARROW SCOPE (review honesty fix): this models ONLY the steady-state subset of
// the host `ProjectionCache` merge (D3-3) that this scenario exercises:
//   - Changed row → overwrite cache[key]
//   - Cleared row → remove cache[key]
//   - absent key  → retain cache[key] (the omit==retain invariant)
//
// It deliberately does NOT model the rebaseline path: no `removeAll` on a
// session/epoch change, no `sessionId == 0` full-snapshot pass-through, no
// rev-monotonicity guard, no decode-before-commit. This scenario never bumps
// session or epoch, so that path is never exercised here — and it is already
// proven by R6-S1's dedicated FrameIdentity rebaseline tests. Modeling it in
// this stand-in would be asserted-by-doc-but-never-run, so it is omitted.

#[derive(Default)]
struct MiniProjectionCache {
    cache: HashMap<String, Vec<u8>>,
}

impl MiniProjectionCache {
    /// Apply one frame's rows: Changed overwrites, Cleared removes, absent keys
    /// retain. (Steady-state subset only — see the section doc above.)
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

// ── Oracle result ─────────────────────────────────────────────────────────────

pub(crate) struct FeedOracleResult {
    pub(crate) passed: bool,
    pub(crate) note: String,
}

/// Keys allowed to be absent from the incremental reconstruction.
///
/// - `refs.event.envelopes`, `nip46_onboarding`: nondeterministic Tier-1 keys
///   (always-Changed by D3-7, no manifest entry; legitimate to differ between
///   the two independent kernel instances).
/// - `nmp.testing.feed_idle`: the feed key IS expected to be present in the
///   reconstruction (from the first Phase B full-baseline tick), so it is NOT
///   in the absent whitelist. Its omission from the reconstruction would mean
///   the first Phase B tick never emitted it — a real bug. See the module doc.
const ALLOWED_ABSENT: &[&str] = &["refs.event.envelopes", "nip46_onboarding"];

/// Run the byte-identity oracle against the Phase B incremental stream.
///
/// Replays all Phase B raw frames through a `MiniProjectionCache` and compares
/// the reconstructed end-state to Phase A's final full-frame projection set.
///
/// Fail-closed verdict:
/// - key present + byte-identical              → OK
/// - key present + payload differs             → hard FAIL (corruption)
/// - key absent + in `ALLOWED_ABSENT`          → tolerated
/// - key absent + NOT in `ALLOWED_ABSENT`      → hard FAIL (dropped row)
///
/// The `nmp.testing.feed_idle` key must be present in the reconstruction (the first
/// Phase B tick is always a full baseline that carries it), so a missing feed
/// key is a hard FAIL.
pub(crate) fn run_feed_oracle(
    incremental_frames: &[Vec<u8>],
    full_frame_records: &[FeedFrameRecord],
) -> FeedOracleResult {
    if incremental_frames.is_empty() || full_frame_records.is_empty() {
        return FeedOracleResult {
            passed: false,
            note: format!(
                "FAIL: insufficient data — incremental_frames={} full_frame_records={}",
                incremental_frames.len(),
                full_frame_records.len()
            ),
        };
    }

    // Replay the full incremental stream.
    let mut cache = MiniProjectionCache::default();
    for frame in incremental_frames {
        cache.merge_frame(frame);
    }
    let reconstructed = cache.cache;

    // Reference: the last Phase A frame's projection set.
    let reference = &full_frame_records[full_frame_records.len() - 1].projection_payloads;

    let mut hard_failures: Vec<String> = Vec::new();
    let mut tolerated: Vec<String> = Vec::new();
    let mut matched = 0usize;

    for (key, ref_payload) in reference {
        if ref_payload.is_empty() {
            continue;
        }
        match reconstructed.get(key) {
            Some(recon) if recon == ref_payload => {
                matched += 1;
            }
            Some(recon) => {
                hard_failures.push(format!(
                    "key='{}' PAYLOAD MISMATCH ref_len={} recon_len={}",
                    key,
                    ref_payload.len(),
                    recon.len()
                ));
            }
            None if ALLOWED_ABSENT.contains(&key.as_str()) => {
                tolerated.push(key.clone());
            }
            None => {
                hard_failures.push(format!(
                    "key='{}' DROPPED (absent from incremental reconstruction, ref_len={}) \
                     — not in the allowed-absent whitelist",
                    key,
                    ref_payload.len()
                ));
            }
        }
    }

    if hard_failures.is_empty() {
        FeedOracleResult {
            passed: true,
            note: format!(
                "PASS (fail-closed) — {matched} keys byte-identical; {} whitelisted absent: {:?}; \
                 {n} incremental frames applied",
                tolerated.len(),
                tolerated,
                n = incremental_frames.len(),
            ),
        }
    } else {
        FeedOracleResult {
            passed: false,
            note: format!(
                "FAIL (fail-closed) — {} hard failure(s): {:?}",
                hard_failures.len(),
                &hard_failures[..hard_failures.len().min(5)],
            ),
        }
    }
}
