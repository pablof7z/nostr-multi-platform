package org.nmp.android

import android.util.Log
import nmp.nip02.FollowListSnapshot as FbFollowListSnapshot
import org.nmp.android.model.FollowListSnapshot
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedFollowListDecoder"

/** Decode the Rust-owned `nmp.follow_list` (`NF02`) typed projection. */
object TypedFollowListDecoder {
    const val PROJECTION_KEY = "nmp.follow_list"
    const val SCHEMA_ID = "nmp.nip02.follow_list"
    const val FILE_IDENTIFIER = "NF02"

    private const val SUPPORTED_SCHEMA_VERSION: UInt = 1u

    fun decode(projections: List<TypedProjectionEnvelope>): FollowListSnapshot? {
        val projection = projections.firstOrNull {
            it.key == PROJECTION_KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.schemaVersion != SUPPORTED_SCHEMA_VERSION) return null
        if (projection.payload.isEmpty()) return null
        return decodeBytes(projection.payload)
    }

    fun decodeBytes(bytes: ByteArray): FollowListSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbFollowListSnapshot.FollowListSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "NF02 file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val root = FbFollowListSnapshot.getRootAsFollowListSnapshot(bb)
            val follows = buildList {
                for (i in 0 until root.followsLength) {
                    val pubkey = root.follows(i)?.pubkey ?: continue
                    add(pubkey)
                }
            }
            FollowListSnapshot(follows = follows)
        } catch (e: Exception) {
            Log.e(TAG, "NF02 decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }
}
