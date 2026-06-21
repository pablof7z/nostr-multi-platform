package org.nmp.android

import android.util.Log
import nmp.kernel.RelayRoleOption as FbRelayRoleOption
import nmp.kernel.RelayRoleOptionsSnapshot
import org.nmp.android.model.RelayRoleOption
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedRelayRoleOptionsDecoder"

/**
 * Typed-first decoder for the kernel-owned `relay_role_options` snapshot
 * projection (`KRRO` / `RelayRoleOptionsSnapshot`) — the Android peer of iOS
 * `TypedRelayRoleOptionsDecoder` (`TypedProjectionDecoders.generated.swift`) +
 * `TypedProjectionGlue.relayRoleOptions`.
 *
 * Field-for-field mirror of `RelayRoleOption { value, tint, is_default }`.
 * `label` was removed from the wire (#1678, D7); it is now a computed shell
 * property derived from `value` in `RelayRoleOption`. No `has_*` companion
 * bools — both strings are always present.
 *
 * Returns `null` when the `KRRO` sidecar is absent / wrong schema /
 * unverifiable, so the typed-only host uses the empty projection default. Fail
 * closed (D1/D6) on a malformed buffer.
 */
object TypedRelayRoleOptionsDecoder {

    const val KEY = "relay_role_options"
    const val SCHEMA_ID = "relay_role_options"
    const val FILE_IDENTIFIER = "KRRO"

    fun decode(projections: List<TypedProjectionEnvelope>): List<RelayRoleOption>? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KRRO` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): List<RelayRoleOption>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!RelayRoleOptionsSnapshot.RelayRoleOptionsSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KRRO file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = RelayRoleOptionsSnapshot.getRootAsRelayRoleOptionsSnapshot(bb)
            buildList {
                for (i in 0 until snapshot.optionsLength) {
                    val opt = snapshot.options(i) ?: continue
                    add(mapOption(opt))
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "KRRO decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    // `label` removed from wire (#1678, D7); `RelayRoleOption.label` is now a
    // computed shell property derived from `value`.
    private fun mapOption(opt: FbRelayRoleOption): RelayRoleOption = RelayRoleOption(
        value = opt.value ?: "",
        tint = opt.tint ?: "",
        isDefault = opt.isDefault,
    )
}
