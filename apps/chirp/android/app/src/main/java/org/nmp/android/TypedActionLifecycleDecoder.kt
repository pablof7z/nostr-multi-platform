package org.nmp.android

import android.util.Log
import nmp.kernel.ActionLifecycleSnapshot as FbActionLifecycleSnapshot
import nmp.kernel.LifecycleEntry as FbLifecycleEntry
import org.nmp.android.model.ActionLifecycleEntry
import org.nmp.android.model.ActionLifecycleSnapshot
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedActionLifecycleDecoder"

/**
 * Typed-first decoder for the kernel-owned `action_lifecycle` snapshot
 * projection (`KALC` / [FbActionLifecycleSnapshot]) — the Android peer of iOS
 * `TypedActionLifecycleDecoder` + `TypedProjectionGlue.actionLifecycle`.
 *
 * Drives the in-flight / recent-terminal action UI (e.g. the Marmot dialog
 * dismissal on a `recentTerminal` "accepted" entry). Maps each
 * [FbLifecycleEntry] row to the Android [ActionLifecycleEntry] domain type
 * field-for-field; `has_reason == false` lifts to `reason = null`
 * (byte-faithful to the JSON `null`-when-absent path).
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent fallback. Returns `null`
 * when the `KALC` sidecar is absent / wrong schema / unverifiable, so the
 * caller keeps `actionLifecycle = null`. An empty-but-present sidecar decodes
 * to a snapshot with two empty lists (NOT `null`). Fail closed (D1) on a
 * malformed buffer.
 */
object TypedActionLifecycleDecoder {

    const val KEY = "action_lifecycle"
    const val SCHEMA_ID = "action_lifecycle"
    const val FILE_IDENTIFIER = "KALC"

    fun decode(projections: List<TypedProjectionEnvelope>): ActionLifecycleSnapshot? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KALC` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): ActionLifecycleSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbActionLifecycleSnapshot.ActionLifecycleSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KALC file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = FbActionLifecycleSnapshot.getRootAsActionLifecycleSnapshot(bb)
            ActionLifecycleSnapshot(
                inFlight = mapEntries(snapshot.inFlightLength) { snapshot.inFlight(it) },
                recentTerminal = mapEntries(snapshot.recentTerminalLength) { snapshot.recentTerminal(it) },
            )
        } catch (e: Exception) {
            Log.e(TAG, "KALC decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private inline fun mapEntries(
        count: Int,
        row: (Int) -> FbLifecycleEntry?,
    ): List<ActionLifecycleEntry> {
        if (count == 0) return emptyList()
        val result = ArrayList<ActionLifecycleEntry>(count)
        for (i in 0 until count) {
            val entry = row(i) ?: continue
            result.add(
                ActionLifecycleEntry(
                    correlationId = entry.correlationId ?: "",
                    stage = entry.stage ?: "",
                    reason = if (entry.hasReason) entry.reason else null,
                    // #1735: lift the curated reason_code (+ subject) when present;
                    // an un-coded failure has hasReasonCode == false -> null.
                    reasonCode = if (entry.hasReasonCode) entry.reasonCode else null,
                    reasonSubject = if (entry.hasReasonSubject) entry.reasonSubject else null,
                )
            )
        }
        return result
    }
}
