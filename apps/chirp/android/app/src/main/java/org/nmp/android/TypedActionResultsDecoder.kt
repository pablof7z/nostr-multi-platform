package org.nmp.android

import android.util.Log
import nmp.kernel.ActionResultsSnapshot as FbActionResultsSnapshot
import org.nmp.android.model.LastActionResult
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedActionResultsDecoder"

/**
 * Typed-first decoder for the kernel-owned `action_results` snapshot projection
 * (`KARS` / [FbActionResultsSnapshot]) — the Android peer of iOS
 * `TypedActionResultsDecoder` + `TypedProjectionGlue.actionResults`.
 *
 * Per-tick drain array: maps each `ActionResult` row to the Android
 * [LastActionResult] domain type field-for-field. The `has_error` companion
 * bool preserves the JSON `null`-when-absent semantics (`error = null` when
 * `has_error == false`). The wire `result` field is not part of the Android
 * [LastActionResult] domain (field-subset), so it is ignored.
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent fallback. Returns `null`
 * when the `KARS` sidecar is absent / wrong schema / unverifiable, so the caller
 * substitutes `emptyList()`. Fail closed (D1) on a malformed buffer.
 */
object TypedActionResultsDecoder {

    const val KEY = "action_results"
    const val SCHEMA_ID = "action_results"
    const val FILE_IDENTIFIER = "KARS"

    fun decode(projections: List<TypedProjectionEnvelope>): List<LastActionResult>? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KARS` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): List<LastActionResult>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbActionResultsSnapshot.ActionResultsSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KARS file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = FbActionResultsSnapshot.getRootAsActionResultsSnapshot(bb)
            val count = snapshot.resultsLength
            if (count == 0) return emptyList()
            val result = ArrayList<LastActionResult>(count)
            for (i in 0 until count) {
                val row = snapshot.results(i) ?: continue
                result.add(
                    LastActionResult(
                        correlationId = row.correlationId ?: "",
                        status = row.status ?: "",
                        error = if (row.hasError) row.error else null,
                    )
                )
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "KARS decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }
}
