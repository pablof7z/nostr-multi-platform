//! ADR-0070 Lane A (#1671) — generated per-key (row-keyed) reference cache for
//! Kotlin (Android). The Android twin of [`crate::swift_keyed_cache`]: it
//! decodes the `nmp.refs.RefRowDeltaBatch` payload and merges row deltas under
//! the five invariants, byte-for-byte semantically identical to the Swift cache
//! and the Rust reference model (`nmp_core::refs::RefRowCache`).
//!
//! Generates `apps/chirp/android/app/src/main/java/org/nmp/android/KeyedRefCache.kt` from
//! [`KEYED_PROJECTIONS`].

use std::path::Path;

use crate::swift_projections_registry::{KeyedProjectionEntry, KEYED_PROJECTIONS};

// ADR-0070 Lane G (#1671): the TYPED row-payload rendering (accessors +
// decode-before-commit seam) lives in a sibling file so neither source exceeds
// the 500-LOC cap (the Swift `swift_keyed_cache_typed.rs` twin).
#[path = "kotlin_keyed_cache_typed.rs"]
mod typed;
use typed::{render_accessors, render_typed_decoders};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen keyed-ref-cache --platform kotlin \\
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/KeyedRefCache.kt
//
// Source of truth: KEYED_PROJECTIONS in
// `crates/nmp-codegen/src/swift_projections_registry.rs`.
// The CI gate (`codegen-drift.yml`) fails any PR whose generated Kotlin differs.
//
// ADR-0070 Lane A (#1671): per-key row cache for keyed reference projections
// (`refs.profile` / `refs.event`) — byte-for-byte semantically identical to
// `KeyedRefCache.generated.swift` and `nmp_core::refs::RefRowCache`.
// ─────────────────────────────────────────────────────────────────────────────

@file:OptIn(ExperimentalUnsignedTypes::class)

package org.nmp.android

import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import nmp.kernel.ClaimedEventsSnapshot
import nmp.kernel.ProfileSnapshot
import nmp.refs.RefRowDeltaBatch
import nmp.refs.RefRowState
import org.nmp.android.model.ProfileCard

private const val KRC_TAG = \"KeyedRefCache\"

";

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct KotlinKeyedRefCacheCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the Kotlin `KeyedRefCache` source from the keyed-projection registry.
#[must_use]
pub fn render_kotlin_keyed_ref_cache(entries: &[KeyedProjectionEntry]) -> String {
    let mut out = String::from(HEADER);
    out.push_str(STATIC_TYPES);
    out.push_str(&render_routing(entries));
    out.push_str(STATIC_MERGE);
    out.push_str(&render_typed_decoders(entries));
    out.push_str(&render_accessors(entries));
    out.push_str("}\n");
    out
}

fn render_routing(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    /** Map a frame's `TypedProjection.key` to its resolver namespace. */\n\
         \x20\x20\x20\x20private fun namespace(forProjectionKey: String): String? = when (forProjectionKey) {\n",
    );
    for e in entries {
        s.push_str(&format!(
            "        {:?} -> {:?}\n",
            e.projection_key, e.namespace
        ));
    }
    s.push_str(
        "        else -> null\n\
         \x20\x20\x20\x20}\n\n",
    );
    s
}

const STATIC_TYPES: &str = r#"/** One cached row: last committed per-key rev + raw typed payload bytes. */
private data class RefRowCacheEntry(val rev: ULong, val payload: ByteArray) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is RefRowCacheEntry) return false
        return rev == other.rev && payload.contentEquals(other.payload)
    }
    override fun hashCode(): Int = 31 * rev.hashCode() + payload.contentHashCode()
}

/** A per-row change: one is delivered to listeners per committed/cleared key. */
data class KeyedRowChange(val projectionKey: String, val rowKey: String, val cleared: Boolean)

/** A row decoded out of the FlatBuffer BEFORE any cache mutation (fail-closed). */
private data class DecodedRefRow(val key: String, val rev: ULong, val state: UByte, val payload: ByteArray)

/**
 * NMP-owned per-key row cache for keyed reference projections (ADR-0070).
 *
 * Thread-safety: fed only from `KernelModel.applyFrame` on the single native
 * update-listener thread, identical to `ProjectionMergeCache`.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class KeyedRefCache {
    // projectionKey -> (rowKey -> entry)
    private val rows = HashMap<String, HashMap<String, RefRowCacheEntry>>()
    private var appliedSession: ULong = 0UL
    private var appliedEpoch: ULong = 0UL
    /** D3-5: false until the first post-baseline frame is applied. */
    var baselined: Boolean = false
        private set
    /** D3-4: latches on any per-row decode-before-commit failure. */
    var needsResync: Boolean = false
        private set
    private val rowChangeListeners = mutableListOf<(KeyedRowChange) -> Unit>()

    /**
     * Decode-before-commit seam (ADR-0070 invariant #2). The per-namespace
     * typed-row validator the cache runs on a `Changed` row's payload BEFORE it
     * replaces a slot: true iff `payload` decodes to the namespace's concrete
     * ref type (`refs.profile` -> ProfileRef, `refs.event` -> EventEmbed). The
     * default accepts any non-empty payload; Lane C injects the real decoder. A
     * row that fails is NOT committed (prior row retained, needsResync latched).
     */
    var rowDecoder: (String, ByteArray) -> Boolean = { _, payload -> payload.isNotEmpty() }

    /**
     * ADR-0070 Lane G (#1671): wire the real typed decode-before-commit seam at
     * construction so every `Changed` row is validated against the namespace's
     * concrete type (no caller setup required) — the Swift `init` twin.
     */
    init {
        installTypedRowDecoder()
    }

    /** Register a per-row change listener (one call per committed/cleared key). */
    fun addRowChangeListener(listener: (KeyedRowChange) -> Unit) {
        rowChangeListeners.add(listener)
    }

    /** Hard-reset so the next frame is a full baseline. */
    fun reset() {
        rows.clear()
        appliedSession = 0UL
        appliedEpoch = 0UL
        baselined = false
        needsResync = false
    }

"#;

const STATIC_MERGE: &str = r#"    /**
     * Merge one keyed-projection payload (`nmp.refs.RefRowDeltaBatch` bytes)
     * under the frame's session/epoch. Returns the row keys whose cached row
     * changed (committed or cleared) this frame.
     *
     * Invariants: absent row == Unchanged (retained); explicit Cleared removes;
     * decode-before-commit per row (malformed keeps prior + latches
     * needsResync); session/epoch change or `baseline` rebuilds the full set.
     */
    fun merge(projectionKey: String, payload: ByteArray, sessionId: ULong, snapshotEpoch: ULong): Set<String> {
        val namespace = namespace(forProjectionKey = projectionKey) ?: return emptySet()

        // D4: an identity (session/epoch) change demands a full rebuild. We do
        // NOT clear here — clearing before the new baseline decodes would empty a
        // live cache on a malformed first frame (fail-open). The reset is
        // DEFERRED into the baseline commit (scratch-then-commit), so the prior
        // cache survives a garbage baseline after an identity bump (BLOCKING-1).
        val identityChanged = sessionId != appliedSession || snapshotEpoch != appliedEpoch

        // Fail-closed CHECKED decode at BATCH grain: verify the `NRRD`
        // file_identifier BEFORE any cache mutation and decode the whole buffer
        // under a guard, so empty / wrong-file-id / structurally-invalid bytes
        // retain the prior cache + latch needsResync rather than throwing.
        if (payload.isEmpty()) {
            needsResync = true
            return emptySet()
        }
        val decoded: List<DecodedRefRow>
        val isBaseline: Boolean
        try {
            val bb = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
            if (payload.size < 8 || !RefRowDeltaBatch.RefRowDeltaBatchBufferHasIdentifier(bb)) {
                needsResync = true
                Log.e(KRC_TAG, "RefRowDeltaBatch missing NRRD identifier for projection=$projectionKey — retaining prior cache, needsResync latched")
                return emptySet()
            }
            val batch = RefRowDeltaBatch.getRootAsRefRowDeltaBatch(bb)
            isBaseline = batch.baseline
            // Decode ALL rows out of the buffer FIRST (decode-before-commit): a
            // garbage offset throws here, before any cache slot is touched.
            // Whole-batch fail-closed (BLOCKING-2/3): a row with NO key, OR an
            // out-of-range `state` discriminant (anything other than Changed=0 /
            // Cleared=1), rejects the ENTIRE batch — prior cache retained,
            // needsResync latched, NOTHING committed. No row is skipped.
            val acc = ArrayList<DecodedRefRow>(batch.rowsLength)
            for (i in 0 until batch.rowsLength) {
                val row = batch.rows(i)
                if (row == null) {
                    needsResync = true
                    Log.e(KRC_TAG, "RefRowDeltaBatch row $i null for projection=$projectionKey — rejecting whole batch, needsResync latched")
                    return emptySet()
                }
                val key = row.key
                if (key == null) {
                    needsResync = true
                    Log.e(KRC_TAG, "RefRowDeltaBatch row $i missing key for projection=$projectionKey — rejecting whole batch, needsResync latched")
                    return emptySet()
                }
                val state = row.state
                if (state > RefRowState.Cleared) {
                    needsResync = true
                    Log.e(KRC_TAG, "RefRowDeltaBatch row $i unknown state=$state for projection=$projectionKey — rejecting whole batch, needsResync latched")
                    return emptySet()
                }
                val bytes = ByteArray(row.payloadLength) { j -> row.payload(j).toByte() }
                acc.add(DecodedRefRow(key, row.rev, state, bytes))
            }
            decoded = acc
        } catch (e: Exception) {
            needsResync = true
            Log.e(KRC_TAG, "malformed RefRowDeltaBatch for projection=$projectionKey — retaining prior cache, needsResync latched: ${e.message}")
            return emptySet()
        }

        return if (isBaseline) {
            applyBaseline(projectionKey, namespace, decoded, identityChanged, sessionId, snapshotEpoch)
        } else {
            // A non-baseline batch under a changed identity cannot rebuild the
            // full set; fail closed (adopt identity, latch resync, retain prior
            // cache) rather than merge deltas onto a stale-epoch base. The
            // producer always follows an identity bump with a baseline frame.
            if (identityChanged) {
                appliedSession = sessionId
                appliedEpoch = snapshotEpoch
                baselined = false
                needsResync = true
                return emptySet()
            }
            applyIncremental(projectionKey, namespace, decoded)
        }
    }

    /**
     * Scratch-then-commit baseline (invariant #3 + decode-before-commit on the
     * WHOLE batch): decode every required row into a scratch map first and
     * replace the projection only after all rows decode. One bad row fails the
     * entire baseline closed — prior cache preserved, needsResync latched.
     *
     * When `identityChanged` is set this is the FIRST baseline at a new
     * session/epoch: on a SUCCESSFUL decode it flips identity and drops every
     * OTHER projection's prior-epoch rows as part of the atomic commit; on a
     * decode FAILURE it touches nothing (BLOCKING-1, fail-closed).
     */
    private fun applyBaseline(projectionKey: String, namespace: String, decoded: List<DecodedRefRow>, identityChanged: Boolean, sessionId: ULong, snapshotEpoch: ULong): Set<String> {
        val scratch = HashMap<String, RefRowCacheEntry>()
        for (row in decoded) {
            if (row.state == RefRowState.Cleared) {
                scratch.remove(row.key)
                continue
            }
            if (row.payload.isEmpty() || !rowDecoder(namespace, row.payload)) {
                needsResync = true
                Log.e(KRC_TAG, "decode-before-commit failed in baseline for projection=$projectionKey key=${row.key} — preserving prior cache, needsResync latched")
                return emptySet()
            }
            val existing = scratch[row.key]
            if (existing != null && row.rev <= existing.rev) continue
            scratch[row.key] = RefRowCacheEntry(row.rev, row.payload)
        }

        // Decode succeeded → now (and ONLY now) it is safe to mutate state. On an
        // identity change this is where the DEFERRED reset lands: adopt the new
        // identity and drop every OTHER projection's prior-epoch rows, then treat
        // the prior-epoch slot as gone so every scratch row counts as new.
        if (identityChanged) {
            rows.keys.retainAll { it == projectionKey }
            appliedSession = sessionId
            appliedEpoch = snapshotEpoch
            needsResync = false
        }

        // Atomic commit: diff prior vs scratch so exactly the changed slots
        // re-render (added / updated / dropped ghost), then swap the projection.
        val prior: Map<String, RefRowCacheEntry> = if (identityChanged) emptyMap() else (rows[projectionKey] ?: emptyMap())
        val changed = mutableSetOf<String>()
        for ((key, entry) in scratch) {
            val priorPayload = prior[key]?.payload
            if (priorPayload == null || !entry.payload.contentEquals(priorPayload)) changed.add(key)
        }
        for (key in prior.keys) {
            if (!scratch.containsKey(key)) changed.add(key)
        }
        rows[projectionKey] = scratch
        baselined = true
        for (key in changed) {
            notifyRowChange(KeyedRowChange(projectionKey, key, cleared = !scratch.containsKey(key)))
        }
        return changed
    }

    /**
     * Steady-state incremental merge with rev-safe clears and the per-row
     * decode-before-commit seam.
     */
    private fun applyIncremental(projectionKey: String, namespace: String, decoded: List<DecodedRefRow>): Set<String> {
        val ns = rows.getOrPut(projectionKey) { HashMap() }
        val changed = mutableSetOf<String>()
        for (row in decoded) {
            if (row.state == RefRowState.Cleared) {
                // Rev-safe clear: remove only if the clear's rev is NEWER than
                // the cached row, so a stale reordered clear can never delete a
                // newer live row. A clear for an absent key is a no-op.
                val cached = ns[row.key]
                if (cached != null && row.rev > cached.rev) {
                    ns.remove(row.key)
                    changed.add(row.key)
                    notifyRowChange(KeyedRowChange(projectionKey, row.key, cleared = true))
                }
                continue
            }
            // Changed. Reorder/duplicate guard: skip a row not newer than cached.
            val cached = ns[row.key]
            if (cached != null && row.rev <= cached.rev) continue
            // Decode-before-commit per row (invariant #2) via the typed seam:
            // empty OR invalid bytes → keep the prior row, latch needsResync.
            if (row.payload.isEmpty() || !rowDecoder(namespace, row.payload)) {
                needsResync = true
                Log.e(KRC_TAG, "decode-before-commit failed for projection=$projectionKey key=${row.key} rev=${row.rev} — keeping prior row, needsResync latched")
                continue
            }
            ns[row.key] = RefRowCacheEntry(row.rev, row.payload)
            changed.add(row.key)
            notifyRowChange(KeyedRowChange(projectionKey, row.key, cleared = false))
        }
        baselined = true
        return changed
    }

    private fun notifyRowChange(change: KeyedRowChange) {
        for (listener in rowChangeListeners) listener(change)
    }

    /**
     * The cached raw payload bytes for one (projectionKey, rowKey), or null.
     *
     * `internal` (NOT public): this is the cache's row-bytes merge primitive, not
     * a public refs API. The PUBLIC per-namespace surface is the TYPED accessor
     * (`profile(key) -> ProfileCard?` / `event(key) -> ClaimedEventDto?`, ADR-0070
     * Lane G), which decodes these bytes through the namespace's typed reader.
     * Visible to same-module tests, never to external callers — no dishonest raw
     * surface.
     */
    internal fun payload(projectionKey: String, rowKey: String): ByteArray? =
        rows[projectionKey]?.get(rowKey)?.payload

    /** The number of cached rows for a projection (test/diagnostic aid). */
    fun count(projectionKey: String): Int = rows[projectionKey]?.size ?: 0

"#;

/// Write the generated Kotlin file to `out_path`.
pub fn generate_kotlin_keyed_ref_cache(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
pub fn check_kotlin_keyed_ref_cache(
    out_path: &Path,
) -> std::io::Result<KotlinKeyedRefCacheCheckOutcome> {
    let rendered = render_kotlin_keyed_ref_cache(KEYED_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(KotlinKeyedRefCacheCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(KotlinKeyedRefCacheCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(KotlinKeyedRefCacheCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "kotlin_keyed_cache_tests.rs"]
mod tests;
