package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Error-path contract tests for [TypedEmbedSidecarDecoder] — the typed-first
 * decode of the `refs.event.envelopes` (`NEMB` / `nmp.embed.RefEventEnvelopes`)
 * sidecar (#1283 / #1335 item 2).
 *
 * Covers the ADR-0037 Commitment 4 fail-closed contract: an absent sidecar,
 * wrong schema id, clobbered file identifier, empty payload, or empty entries
 * vector must all decode to an empty map — never a crash, never stale values
 * (D1). The positive round-trips for each projection kind live in the sibling
 * files [TypedEmbedSidecarDecoderShortNoteTest] and
 * [TypedEmbedSidecarDecoderKindsTest]; shared FlatBuffers fixtures live in
 * [NembTestFixtures].
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedEmbedSidecarDecoderTest {

    @Test
    fun absentSidecarReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(emptyList()))
    }

    @Test
    fun emptyPayloadReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(ByteArray(0)))
    }

    @Test
    fun wrongSchemaIdIsIgnored() {
        val env = TypedProjectionEnvelope(
            key = TypedEmbedSidecarDecoder.PROJECTION_KEY,
            schemaId = "wrong.schema",
            schemaVersion = 1u,
            fileIdentifier = TypedEmbedSidecarDecoder.FILE_IDENTIFIER,
            payload = NembTestFixtures.emptyNembBuffer(),
        )
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(listOf(env)))
    }

    @Test
    fun wrongFileIdentifierReturnsEmptyMap() {
        val garbled = NembTestFixtures.emptyNembBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber the NEMB file identifier
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(garbled))
    }

    @Test
    fun emptyEntriesVectorReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(NembTestFixtures.emptyNembBuffer()))
    }
}
