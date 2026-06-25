package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.nip02.FollowEntry as FbFollowEntry
import nmp.nip02.FollowListSnapshot as FbFollowListSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Tests for the Android decode path for `nmp.follow_list` (`NF02`).
 *
 * The profile screen consumes this Rust-owned projection for button state; the
 * shell must not keep its own optimistic follow cache.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedFollowListDecoderTest {

    @Test
    fun absentSidecarReturnsNull() {
        assertNull(TypedFollowListDecoder.decode(emptyList()))
    }

    @Test
    fun emptyPayloadReturnsNull() {
        assertNull(TypedFollowListDecoder.decodeBytes(ByteArray(0)))
    }

    @Test
    fun wrongFileIdentifierReturnsNull() {
        val garbled = followListBuffer(listOf("aa")).copyOf()
        garbled[4] = 'X'.code.toByte()
        assertNull(TypedFollowListDecoder.decodeBytes(garbled))
    }

    @Test
    fun happyPathDecodesFollowsInOrder() {
        val out = requireNotNull(TypedFollowListDecoder.decodeBytes(followListBuffer(listOf("aa", "bb"))))
        assertEquals(listOf("aa", "bb"), out.follows)
    }

    @Test
    fun selectsByKeySchemaAndVersion() {
        val env = envelope(followListBuffer(listOf("aa")))

        assertNull(TypedFollowListDecoder.decode(listOf(env.copy(key = "other"))))
        assertNull(TypedFollowListDecoder.decode(listOf(env.copy(schemaId = "other"))))
        assertNull(TypedFollowListDecoder.decode(listOf(env.copy(schemaVersion = 2u))))

        assertEquals(listOf("aa"), requireNotNull(TypedFollowListDecoder.decode(listOf(env))).follows)
    }

    @Test
    fun kernelProjectionWiringPopulatesSnapshotProjections() {
        val env = envelope(followListBuffer(listOf("aa", "bb")))
        val projections = KernelUpdateFrameDecoder.decodeProjections(listOf(env))

        assertEquals(listOf("aa", "bb"), projections.followList.follows)
    }

    @Test
    fun kernelProjectionWiringFallsBackToEmptyOnMalformedSidecar() {
        val garbled = followListBuffer(listOf("aa")).copyOf()
        garbled[4] = 'X'.code.toByte()

        val projections = KernelUpdateFrameDecoder.decodeProjections(listOf(envelope(garbled)))

        assertEquals(emptyList<String>(), projections.followList.follows)
    }

    private fun envelope(
        payload: ByteArray,
        key: String = TypedFollowListDecoder.PROJECTION_KEY,
        schemaId: String = TypedFollowListDecoder.SCHEMA_ID,
    ): TypedProjectionEnvelope = TypedProjectionEnvelope(
        key = key,
        schemaId = schemaId,
        schemaVersion = 1u,
        fileIdentifier = TypedFollowListDecoder.FILE_IDENTIFIER,
        payload = payload,
    )

    private fun followListBuffer(pubkeys: List<String>): ByteArray {
        val builder = FlatBufferBuilder(256)
        val rows = pubkeys.map { pubkey ->
            val pubkeyOffset = builder.createString(pubkey)
            FbFollowEntry.createFollowEntry(builder, pubkeyOffset)
        }.toIntArray()
        val followsVector = FbFollowListSnapshot.createFollowsVector(builder, rows)
        val root = FbFollowListSnapshot.createFollowListSnapshot(builder, followsVector)
        FbFollowListSnapshot.finishFollowListSnapshotBuffer(builder, root)
        return builder.sizedByteArray()
    }
}
