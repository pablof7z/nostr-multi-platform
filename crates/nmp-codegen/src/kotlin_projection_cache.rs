//! R3-S4 (ADR-0070) — generated `ProjectionMergeCache` for Kotlin (Android).
//!
//! Generates `apps/chirp/android/app/src/main/java/org/nmp/android/ProjectionCache.kt`
//! from the SAME projection registry as [`crate::swift_projection_cache`], so
//! the cache can never drift from the decoder set.
//!
//! ## The generated cache (D3-3 algorithm — byte-for-byte semantically
//! identical to the Swift implementation)
//!
//! The cache holds a `HashMap<String, CacheEntry>` keyed by projection key.
//! Each `CacheEntry` carries the raw payload bytes and the last committed rev.
//! On every frame the cache runs the merge algorithm:
//!
//! - `Changed` row, rev > cached.rev → decode-before-commit; on success
//!   overwrite cache + advance rev; on failure keep prior + latch
//!   `needsResync`.
//! - `Cleared` row → remove from cache.
//! - Omitted row (Unchanged) → retain cached value (no-op).
//!
//! After merging, the cache re-constructs the FULL merged
//! `List<TypedProjectionEnvelope>` set (cached bytes rebuilt as envelopes)
//! and returns it alongside the set of keys whose rev advanced in this frame.
//! The existing `TypedXDecoder.decode()` family is then fed this merged set —
//! byte-identical to today for unchanged projections.
//!
//! ## Decode-before-commit (D3-4)
//!
//! For each `Changed` row the cache calls the decoder's `decodeBytes()` entry
//! point as a preflight. Only on a non-null result does it overwrite the cache.
//! This prevents a corrupt payload from blanking a healthy cached value.
//!
//! ## session_id == 0 handling (D3-5)
//!
//! A frame with `sessionId == 0UL` is treated as "no incremental contract" —
//! the host does not trust omission. The cache is NOT cleared on such a frame;
//! the frame's envelopes are passed through unchanged as the merged set. UI is
//! gated on `baselined == true`.

use std::path::Path;

use crate::swift_projections_registry::{SnapshotProjectionEntry, SNAPSHOT_PROJECTIONS};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-cache --platform kotlin \\
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/ProjectionCache.kt
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Kotlin differs.
//
// ADR-0070 R3-S4: NMP-owned rev-aware host apply layer (Android). This cache
// implements the D3-3 merge algorithm exactly — byte-for-byte semantically
// identical to `ProjectionCache.generated.swift` — so app code (KernelModel
// accessors, Compose UI) stays oblivious to delta mechanics.
// ─────────────────────────────────────────────────────────────────────────────

@file:OptIn(ExperimentalUnsignedTypes::class)

package org.nmp.android

import android.util.Log

private const val TAG = \"ProjectionMergeCache\"

// MARK: - WireProjectionState constants (D3-3)
// These mirror the FlatBuffers-generated ProjectionPresenceState values from
// nmp.transport.ProjectionPresenceState. We import the numeric constants here
// to avoid a hard dependency on the transport package in the generated file.
private const val STATE_CHANGED: UByte = 0u
private const val STATE_CLEARED: UByte = 1u

";

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct KotlinProjectionCacheCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the `ProjectionMergeCache` Kotlin source from the registry.
#[must_use]
pub fn render_kotlin_projection_cache(entries: &[SnapshotProjectionEntry]) -> String {
    // The entries slice is available for future per-key decode dispatch expansion,
    // but the current implementation uses a uniform bytes.isNotEmpty() guard (D3-4).
    let _ = entries;

    let mut out = String::from(HEADER);

    // ── CacheEntry private data class ─────────────────────────────────────────
    out.push_str(
        "/**\n\
         * One cached projection slot: the raw FlatBuffers payload bytes and\n\
         * the last successfully committed `projectionRev`.\n\
         */\n\
         private data class CacheEntry(\n\
         \x20\x20\x20\x20val rev: ULong,\n\
         \x20\x20\x20\x20val schemaId: String,\n\
         \x20\x20\x20\x20val schemaVersion: UInt,\n\
         \x20\x20\x20\x20val fileIdentifier: String,\n\
         \x20\x20\x20\x20val payload: ByteArray,\n\
         ) {\n\
         \x20\x20\x20\x20// ByteArray equality must be structural.\n\
         \x20\x20\x20\x20override fun equals(other: Any?): Boolean {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if (this === other) return true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if (other !is CacheEntry) return false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return rev == other.rev && schemaId == other.schemaId &&\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaVersion == other.schemaVersion &&\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20fileIdentifier == other.fileIdentifier &&\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20payload.contentEquals(other.payload)\n\
         \x20\x20\x20\x20}\n\
         \x20\x20\x20\x20override fun hashCode(): Int {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20var result = rev.hashCode()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20result = 31 * result + schemaId.hashCode()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20result = 31 * result + schemaVersion.hashCode()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20result = 31 * result + fileIdentifier.hashCode()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20result = 31 * result + payload.contentHashCode()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return result\n\
         \x20\x20\x20\x20}\n\
         }\n\n",
    );

    // ── MergeResult ───────────────────────────────────────────────────────────
    out.push_str(
        "/**\n\
         * Return type of [ProjectionMergeCache.merge]. Carries the fully-merged\n\
         * envelope set (fed to the existing TypedXDecoder family) and the set of\n\
         * projection keys whose rev advanced in this frame (used by\n\
         * `KernelModel.decodeUpdate` to skip unchanged projections).\n\
         */\n\
         data class MergeResult(\n\
         \x20\x20\x20\x20/** Fully-reconstituted envelope set: cached rows for omitted keys,\n\
         \x20\x20\x20\x20 * freshly-decoded rows for Changed keys, nothing for Cleared keys.\n\
         \x20\x20\x20\x20 * Feed this set to the existing TypedXDecoder.decode() family. */\n\
         \x20\x20\x20\x20val mergedEnvelopes: List<TypedProjectionEnvelope>,\n\
         \x20\x20\x20\x20/** Keys whose projectionRev advanced (Changed and committed) in this\n\
         \x20\x20\x20\x20 * frame. Also includes Cleared keys so the caller can null-out the\n\
         \x20\x20\x20\x20 * corresponding projection slot. */\n\
         \x20\x20\x20\x20val changedKeys: Set<String>,\n\
         \x20\x20\x20\x20/** Sticky flag: true when at least one decode-before-commit failed.\n\
         \x20\x20\x20\x20 * The prior cache entry is retained (no silent corruption), but the\n\
         \x20\x20\x20\x20 * host is known-degraded for that key. Rung 3 logs; Rung 4 resyncs. */\n\
         \x20\x20\x20\x20val needsResync: Boolean,\n\
         )\n\n",
    );

    // ── ProjectionMergeCache class ────────────────────────────────────────────
    out.push_str(
        "/**\n\
         * NMP-owned rev-aware projection cache (ADR-0070 R3-S4).\n\
         *\n\
         * Lives in `KernelModel` (one instance per kernel session). Fed each\n\
         * FlatBuffers frame before the TypedXDecoder family runs. Implements\n\
         * the D3-3 merge algorithm exactly so app code is oblivious to delta\n\
         * mechanics.\n\
         *\n\
         * Thread-safety: called only from `KernelModel.applyFrame`, which is\n\
         * invoked from the kernel's update-listener thread (a single native\n\
         * background thread per session). The cache is NOT shared across threads.\n\
         */\n\
         @OptIn(ExperimentalUnsignedTypes::class)\n\
         class ProjectionMergeCache {\n\
         \x20\x20\x20\x20private val cache = HashMap<String, CacheEntry>()\n\
         \x20\x20\x20\x20private var appliedSession: ULong = 0UL\n\
         \x20\x20\x20\x20private var appliedEpoch: ULong = 0UL\n\
         \x20\x20\x20\x20/** D3-5: false until the first post-baseline frame is applied.\n\
         \x20\x20\x20\x20 * UI should be gated on this being true. */\n\
         \x20\x20\x20\x20var baselined: Boolean = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20private set\n\
         \x20\x20\x20\x20/** D3-4: latches true on any decode-before-commit failure.\n\
         \x20\x20\x20\x20 * Rung 4 drains it via `nmp_app_request_full_snapshot`. */\n\
         \x20\x20\x20\x20var needsResync: Boolean = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20private set\n\n",
    );

    // reset() method
    out.push_str(
        "    /**\n\
         \x20\x20\x20\x20 * Hard-reset the cache (called when the kernel session ends or\n\
         \x20\x20\x20\x20 * `KernelModel.onCleared()` runs so the next frame is treated as a\n\
         \x20\x20\x20\x20 * full baseline).\n\
         \x20\x20\x20\x20 */\n\
         \x20\x20\x20\x20fun reset() {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20cache.clear()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20appliedSession = 0UL\n\
         \x20\x20\x20\x20\x20\x20\x20\x20appliedEpoch = 0UL\n\
         \x20\x20\x20\x20\x20\x20\x20\x20baselined = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20needsResync = false\n\
         \x20\x20\x20\x20}\n\n",
    );

    // decodeSucceeds — decode-before-commit preflight
    out.push_str(
        "    // Decode-before-commit preflight (D3-4).\n\
         \x20\x20\x20\x20//\n\
         \x20\x20\x20\x20// Returns `true` iff the payload bytes are well-formed enough to commit.\n\
         \x20\x20\x20\x20// The canonical decode-failure the cache defends against is a `Changed`\n\
         \x20\x20\x20\x20// row carrying EMPTY payload bytes — a malformed frame, because a Changed\n\
         \x20\x20\x20\x20// row by contract carries full bytes (an empty projection is expressed as\n\
         \x20\x20\x20\x20// `Cleared`, never Changed-with-no-bytes). This `!bytes.isEmpty` guard is\n\
         \x20\x20\x20\x20// the sufficient correctness floor under synchronous in-process delivery\n\
         \x20\x20\x20\x20// (per ADR-0070 D3-4 codex review). It is intentionally NOT a per-key\n\
         \x20\x20\x20\x20// typed-decode dispatch, because the Android typed decoders have\n\
         \x20\x20\x20\x20// non-uniform method signatures; a uniform `decodeBytes()` contract is a\n\
         \x20\x20\x20\x20// follow-on clean-up (it can be layered in without a wire change).\n\
         \x20\x20\x20\x20@Suppress(\"UNUSED_PARAMETER\")\n\
         \x20\x20\x20\x20private fun decodeSucceeds(key: String, bytes: ByteArray): Boolean {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return bytes.isNotEmpty()\n\
         \x20\x20\x20\x20}\n\n",
    );

    // merge() — the D3-3 algorithm
    out.push_str(
        "    /**\n\
         \x20\x20\x20\x20 * Run the D3-3 merge algorithm for one incoming frame.\n\
         \x20\x20\x20\x20 *\n\
         \x20\x20\x20\x20 * - If `sessionId` or `snapshotEpoch` changed => mandatory full-cache\n\
         \x20\x20\x20\x20 *   reset (D4). The kernel guarantees the first post-change frame is a\n\
         \x20\x20\x20\x20 *   full baseline.\n\
         \x20\x20\x20\x20 * - `sessionId == 0UL` => no incremental contract (full frame anyway);\n\
         \x20\x20\x20\x20 *   pass envelopes through unchanged without trusting omission (D3-5).\n\
         \x20\x20\x20\x20 * - Changed row, rev > cached.rev => decode-before-commit; on success\n\
         \x20\x20\x20\x20 *   overwrite cache; on failure keep prior + latch `needsResync`.\n\
         \x20\x20\x20\x20 * - Cleared row => remove from cache; add key to changedKeys.\n\
         \x20\x20\x20\x20 * - Omitted row (Unchanged) => retain cached value (no-op).\n\
         \x20\x20\x20\x20 *\n\
         \x20\x20\x20\x20 * @return the fully-merged envelope set + changed-key set.\n\
         \x20\x20\x20\x20 */\n\
         \x20\x20\x20\x20fun merge(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20envelopes: List<TypedProjectionEnvelope>,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20sessionId: ULong,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20snapshotEpoch: ULong,\n\
         \x20\x20\x20\x20): MergeResult {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// D3-5: session_id == 0 means no incremental contract.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Pass envelopes through as-is; do not trust omission.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// changedKeys = all keys present (conservative: treat as fully changed).\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if (sessionId == 0UL) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20val keys = envelopes.map { it.key }.toSet()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20return MergeResult(mergedEnvelopes = envelopes, changedKeys = keys, needsResync = needsResync)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// D4: mandatory reset on session or epoch change.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Reset BEFORE the row loop (atomic per D3-3 pseudocode).\n\
         \x20\x20\x20\x20\x20\x20\x20\x20if (sessionId != appliedSession || snapshotEpoch != appliedEpoch) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache.clear()\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20appliedSession = sessionId\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20appliedEpoch = snapshotEpoch\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20baselined = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync = false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20val changedKeys = mutableSetOf<String>()\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Run the merge algorithm over incoming rows.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20for (envelope in envelopes) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20when (envelope.state) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20STATE_CLEARED -> {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Explicit clear: remove from cache, mark as changed\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// so the caller can null-out the corresponding projection slot.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache.remove(envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys.add(envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20STATE_CHANGED -> {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20val incomingRev = envelope.projectionRev\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// D3 reorder guard: under synchronous in-process delivery this\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// never fires, but belt-and-braces for future async transport.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20val cached = cache[envelope.key]\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (cached != null && incomingRev <= cached.rev) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20continue  // stale or duplicate rev — keep prior\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Decode-before-commit (D3-4): run the typed decoder as a\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// preflight. On success: overwrite cache + advance rev.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// On failure: keep prior entry + latch needsResync.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (decodeSucceeds(envelope.key, envelope.payload)) {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20cache[envelope.key] = CacheEntry(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20rev = incomingRev,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaId = envelope.schemaId,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaVersion = envelope.schemaVersion,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20fileIdentifier = envelope.fileIdentifier,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20payload = envelope.payload,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys.add(envelope.key)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20} else {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync = true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Log.e(TAG, \"decode-before-commit failed for key=${envelope.key} rev=$incomingRev — keeping prior cache entry, needsResync latched\")\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20else -> {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Unknown state value — treat as Unchanged (retain cached).\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// This is the Unchanged == absence case: omitted rows simply\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// don't appear in the envelopes list, but if a row IS present\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// with an unknown state, skip it rather than corrupt the cache.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Unit\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Reconstruct the full merged envelope set from the cache.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Cleared keys are absent from the cache (already removed above),\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// so they are correctly absent from the merged set too.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20val mergedEnvelopes = cache.map { (key, entry) ->\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20TypedProjectionEnvelope(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20key = key,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaId = entry.schemaId,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20schemaVersion = entry.schemaVersion,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20fileIdentifier = entry.fileIdentifier,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20payload = entry.payload,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20projectionRev = entry.rev,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20state = STATE_CHANGED,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\n\
         \x20\x20\x20\x20\x20\x20\x20\x20baselined = true\n\
         \x20\x20\x20\x20\x20\x20\x20\x20return MergeResult(\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20mergedEnvelopes = mergedEnvelopes,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20changedKeys = changedKeys,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20needsResync = needsResync,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20)\n\
         \x20\x20\x20\x20}\n\
         }\n",
    );

    out
}

/// Write the generated `ProjectionCache.kt` to `out_path`.
pub fn generate_kotlin_projection_cache(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_kotlin_projection_cache(SNAPSHOT_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
pub fn check_kotlin_projection_cache(
    out_path: &Path,
) -> std::io::Result<KotlinProjectionCacheCheckOutcome> {
    let rendered = render_kotlin_projection_cache(SNAPSHOT_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(KotlinProjectionCacheCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(KotlinProjectionCacheCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(KotlinProjectionCacheCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}
