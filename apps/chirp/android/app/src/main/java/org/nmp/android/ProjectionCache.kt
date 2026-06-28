// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-cache --platform kotlin \
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/ProjectionCache.kt
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Kotlin differs.
//
// ADR-0055 R3-S4: NMP-owned rev-aware host apply layer (Android). This cache
// implements the D3-3 merge algorithm exactly — byte-for-byte semantically
// identical to `ProjectionCache.generated.swift` — so app code (KernelModel
// accessors, Compose UI) stays oblivious to delta mechanics.
// ─────────────────────────────────────────────────────────────────────────────

@file:OptIn(ExperimentalUnsignedTypes::class)

package org.nmp.android

import android.util.Log

private const val TAG = "ProjectionMergeCache"

// MARK: - WireProjectionState constants (D3-3)
// These mirror the FlatBuffers-generated ProjectionPresenceState values from
// nmp.transport.ProjectionPresenceState. We import the numeric constants here
// to avoid a hard dependency on the transport package in the generated file.
private const val STATE_CHANGED: UByte = 0u
private const val STATE_CLEARED: UByte = 1u

/**
* One cached projection slot: the raw FlatBuffers payload bytes and
* the last successfully committed `projectionRev`.
*/
private data class CacheEntry(
    val rev: ULong,
    val schemaId: String,
    val schemaVersion: UInt,
    val fileIdentifier: String,
    val payload: ByteArray,
) {
    // ByteArray equality must be structural.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is CacheEntry) return false
        return rev == other.rev && schemaId == other.schemaId &&
            schemaVersion == other.schemaVersion &&
            fileIdentifier == other.fileIdentifier &&
            payload.contentEquals(other.payload)
    }
    override fun hashCode(): Int {
        var result = rev.hashCode()
        result = 31 * result + schemaId.hashCode()
        result = 31 * result + schemaVersion.hashCode()
        result = 31 * result + fileIdentifier.hashCode()
        result = 31 * result + payload.contentHashCode()
        return result
    }
}

/**
* Return type of [ProjectionMergeCache.merge]. Carries the fully-merged
* envelope set (fed to the existing TypedXDecoder family) and the set of
* projection keys whose rev advanced in this frame (used by
* `KernelModel.decodeUpdate` to skip unchanged projections).
*/
data class MergeResult(
    /** Fully-reconstituted envelope set: cached rows for omitted keys,
     * freshly-decoded rows for Changed keys, nothing for Cleared keys.
     * Feed this set to the existing TypedXDecoder.decode() family. */
    val mergedEnvelopes: List<TypedProjectionEnvelope>,
    /** Keys whose projectionRev advanced (Changed and committed) in this
     * frame. Also includes Cleared keys so the caller can null-out the
     * corresponding projection slot. */
    val changedKeys: Set<String>,
    /** Sticky flag: true when at least one decode-before-commit failed.
     * The prior cache entry is retained (no silent corruption), but the
     * host is known-degraded for that key. Rung 3 logs; Rung 4 resyncs. */
    val needsResync: Boolean,
)

/**
* NMP-owned rev-aware projection cache (ADR-0055 R3-S4).
*
* Lives in `KernelModel` (one instance per kernel session). Fed each
* FlatBuffers frame before the TypedXDecoder family runs. Implements
* the D3-3 merge algorithm exactly so app code is oblivious to delta
* mechanics.
*
* Thread-safety: called only from `KernelModel.applyFrame`, which is
* invoked from the kernel's update-listener thread (a single native
* background thread per session). The cache is NOT shared across threads.
*/
@OptIn(ExperimentalUnsignedTypes::class)
class ProjectionMergeCache {
    private val cache = HashMap<String, CacheEntry>()
    private var appliedSession: ULong = 0UL
    private var appliedEpoch: ULong = 0UL
    /** D3-5: false until the first post-baseline frame is applied.
     * UI should be gated on this being true. */
    var baselined: Boolean = false
        private set
    /** D3-4: latches true on any decode-before-commit failure.
     * Rung 4 drains it via `nmp_app_request_full_snapshot`. */
    var needsResync: Boolean = false
        private set

    /**
     * Hard-reset the cache (called when the kernel session ends or
     * `KernelModel.onCleared()` runs so the next frame is treated as a
     * full baseline).
     */
    fun reset() {
        cache.clear()
        appliedSession = 0UL
        appliedEpoch = 0UL
        baselined = false
        needsResync = false
    }

    // Decode-before-commit preflight (D3-4).
    //
    // Returns `true` iff the payload bytes are well-formed enough to commit.
    // The canonical decode-failure the cache defends against is a `Changed`
    // row carrying EMPTY payload bytes — a malformed frame, because a Changed
    // row by contract carries full bytes (an empty projection is expressed as
    // `Cleared`, never Changed-with-no-bytes). This `!bytes.isEmpty` guard is
    // the sufficient correctness floor under synchronous in-process delivery
    // (per ADR-0055 D3-4 codex review). It is intentionally NOT a per-key
    // typed-decode dispatch, because the Android typed decoders have
    // non-uniform method signatures; a uniform `decodeBytes()` contract is a
    // follow-on clean-up (it can be layered in without a wire change).
    @Suppress("UNUSED_PARAMETER")
    private fun decodeSucceeds(key: String, bytes: ByteArray): Boolean {
        return bytes.isNotEmpty()
    }

    /**
     * Run the D3-3 merge algorithm for one incoming frame.
     *
     * - If `sessionId` or `snapshotEpoch` changed => mandatory full-cache
     *   reset (D4). The kernel guarantees the first post-change frame is a
     *   full baseline.
     * - `sessionId == 0UL` => no incremental contract (full frame anyway);
     *   pass envelopes through unchanged without trusting omission (D3-5).
     * - Changed row, rev > cached.rev => decode-before-commit; on success
     *   overwrite cache; on failure keep prior + latch `needsResync`.
     * - Cleared row => remove from cache; add key to changedKeys.
     * - Omitted row (Unchanged) => retain cached value (no-op).
     *
     * @return the fully-merged envelope set + changed-key set.
     */
    fun merge(
        envelopes: List<TypedProjectionEnvelope>,
        sessionId: ULong,
        snapshotEpoch: ULong,
    ): MergeResult {
        // D3-5: session_id == 0 means no incremental contract.
        // Pass envelopes through as-is; do not trust omission.
        // changedKeys = all keys present (conservative: treat as fully changed).
        if (sessionId == 0UL) {
            val keys = envelopes.map { it.key }.toSet()
            return MergeResult(mergedEnvelopes = envelopes, changedKeys = keys, needsResync = needsResync)
        }

        // D4: mandatory reset on session or epoch change.
        // Reset BEFORE the row loop (atomic per D3-3 pseudocode).
        if (sessionId != appliedSession || snapshotEpoch != appliedEpoch) {
            cache.clear()
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            baselined = false
            needsResync = false
        }

        val changedKeys = mutableSetOf<String>()

        // Run the merge algorithm over incoming rows.
        for (envelope in envelopes) {
            when (envelope.state) {
                STATE_CLEARED -> {
                    // Explicit clear: remove from cache, mark as changed
                    // so the caller can null-out the corresponding projection slot.
                    cache.remove(envelope.key)
                    changedKeys.add(envelope.key)
                }
                STATE_CHANGED -> {
                    val incomingRev = envelope.projectionRev
                    // D3 reorder guard: under synchronous in-process delivery this
                    // never fires, but belt-and-braces for future async transport.
                    val cached = cache[envelope.key]
                    if (cached != null && incomingRev <= cached.rev) {
                        continue  // stale or duplicate rev — keep prior
                    }
                    // Decode-before-commit (D3-4): run the typed decoder as a
                    // preflight. On success: overwrite cache + advance rev.
                    // On failure: keep prior entry + latch needsResync.
                    if (decodeSucceeds(envelope.key, envelope.payload)) {
                        cache[envelope.key] = CacheEntry(
                            rev = incomingRev,
                            schemaId = envelope.schemaId,
                            schemaVersion = envelope.schemaVersion,
                            fileIdentifier = envelope.fileIdentifier,
                            payload = envelope.payload,
                        )
                        changedKeys.add(envelope.key)
                    } else {
                        needsResync = true
                        Log.e(TAG, "decode-before-commit failed for key=${envelope.key} rev=$incomingRev — keeping prior cache entry, needsResync latched")
                    }
                }
                else -> {
                    // Unknown state value — treat as Unchanged (retain cached).
                    // This is the Unchanged == absence case: omitted rows simply
                    // don't appear in the envelopes list, but if a row IS present
                    // with an unknown state, skip it rather than corrupt the cache.
                    Unit
                }
            }
        }

        // Reconstruct the full merged envelope set from the cache.
        // Cleared keys are absent from the cache (already removed above),
        // so they are correctly absent from the merged set too.
        val mergedEnvelopes = cache.map { (key, entry) ->
            TypedProjectionEnvelope(
                key = key,
                schemaId = entry.schemaId,
                schemaVersion = entry.schemaVersion,
                fileIdentifier = entry.fileIdentifier,
                payload = entry.payload,
                projectionRev = entry.rev,
                state = STATE_CHANGED,
            )
        }

        baselined = true
        return MergeResult(
            mergedEnvelopes = mergedEnvelopes,
            changedKeys = changedKeys,
            needsResync = needsResync,
        )
    }
}
