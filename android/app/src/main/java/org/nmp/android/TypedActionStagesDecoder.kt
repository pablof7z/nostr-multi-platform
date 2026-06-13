package org.nmp.android

import android.util.Log
import nmp.kernel.ActionStagesSnapshot as FbActionStagesSnapshot
import org.nmp.android.model.ActionStageEntry
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedActionStagesDecoder"

/**
 * Typed-first decoder for the kernel-owned `action_stages` snapshot projection
 * (`KAST` / [FbActionStagesSnapshot]) — the Android peer of iOS
 * `TypedActionStagesDecoder` + `TypedProjectionGlue.actionStages`.
 *
 * FlatBuffers has no map type, so the producer flattens the
 * `correlation_id -> [ActionStageEntry]` map to a flat vector of
 * `ActionStagesEntry` rows (one per correlation_id, each with its own `stages`
 * vector). This rebuilds the `Map<String, List<ActionStageEntry>>` the JSON
 * `projections.action_stages` path yields, mirroring the flattened-map
 * precedent (claimed_profiles / marmot_messages).
 *
 * The Android [ActionStageEntry] domain type is a field-subset: it carries the
 * raw `stage` string + `atMs` + optional `reason` (the wire's `detail` field is
 * not part of the Android domain, so it is ignored). `has_reason == false`
 * lifts to `reason = null`, byte-faithful to the JSON path.
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent fallback. Returns `null`
 * when the `KAST` sidecar is absent / wrong schema / unverifiable, so the caller
 * substitutes `emptyMap()`. Fail closed (D1) on a malformed buffer.
 */
object TypedActionStagesDecoder {

    const val KEY = "action_stages"
    const val SCHEMA_ID = "action_stages"
    const val FILE_IDENTIFIER = "KAST"

    fun decode(projections: List<TypedProjectionEnvelope>): Map<String, List<ActionStageEntry>>? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KAST` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): Map<String, List<ActionStageEntry>>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbActionStagesSnapshot.ActionStagesSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KAST file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = FbActionStagesSnapshot.getRootAsActionStagesSnapshot(bb)
            val count = snapshot.entriesLength
            if (count == 0) return emptyMap()
            val result = LinkedHashMap<String, List<ActionStageEntry>>(count * 2)
            for (i in 0 until count) {
                val entry = snapshot.entries(i) ?: continue
                val key = entry.key ?: continue
                val stageCount = entry.stagesLength
                val stages = ArrayList<ActionStageEntry>(stageCount)
                for (j in 0 until stageCount) {
                    val stage = entry.stages(j) ?: continue
                    stages.add(
                        ActionStageEntry(
                            stage = stage.stage ?: "",
                            atMs = stage.atMs.toLong(),
                            reason = if (stage.hasReason) stage.reason else null,
                        )
                    )
                }
                result[key] = stages
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "KAST decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }
}
