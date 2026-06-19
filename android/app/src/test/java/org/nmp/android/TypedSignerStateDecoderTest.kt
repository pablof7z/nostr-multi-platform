package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.SignerState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for [TypedSignerStateDecoder] — the typed-first decode of the
 * kernel-owned `signer_state` (`KSST`) snapshot projection (#1099, ADR-0048).
 * This is the sidecar that drives the signer health badge; Android never wired
 * it before, so the badge was permanently broken. Coverage mirrors the wallet
 * decoder's contract:
 *  - absent sidecar → null;
 *  - wrong file identifier → null;
 *  - a hand-crafted KSST buffer decodes to the correct domain fields;
 *  - the isReady flag is set correctly;
 *  - #1493 P9: the English label / semantic tone are NOT on the wire — the
 *    decoder derives them from the raw `state` token (labels-to-shells).
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedSignerStateDecoderTest {

    @Test
    fun absentSidecarReturnsNull() {
        assertNull(TypedSignerStateDecoder.decode(emptyList()))
    }

    @Test
    fun emptyPayloadReturnsNull() {
        assertNull(TypedSignerStateDecoder.decode(ByteArray(0)))
    }

    @Test
    fun wrongFileIdentifierReturnsNull() {
        val garbled = readyBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KSST identifier
        assertNull(TypedSignerStateDecoder.decode(garbled))
    }

    @Test
    fun happyPathDecodesAllFields() {
        val out = requireNotNull(TypedSignerStateDecoder.decode(readyBuffer()))
        assertEquals("nip46", out.signerKind)
        assertEquals("ready", out.state)
        assertNull(out.reason)
        assertTrue(out.isReady)
        assertFalse(out.isAwaitingApproval)
        assertFalse(out.isReconnecting)
        assertFalse(out.isUnavailable)
        assertFalse(out.isFailed)
    }

    @Test
    fun labelAndToneAreShellDerivedFromState() {
        // #1493 P9: the wire carries only the raw `state` token; the decoder is
        // the shell renderer that derives the English label + semantic tone.
        val out = requireNotNull(TypedSignerStateDecoder.decode(readyBuffer()))
        assertEquals("Connected", out.statusLabel)
        assertEquals("active", out.statusTone)
    }

    @Test
    fun awaitingApprovalCarriesReasonAndAmberTone() {
        val out = requireNotNull(TypedSignerStateDecoder.decode(awaitingBuffer()))
        assertEquals("nip55", out.signerKind)
        assertEquals("awaiting_approval", out.state)
        assertTrue(out.isAwaitingApproval)
        assertFalse(out.isReady)
        assertEquals("approve in Amber", out.reason)
        // Shell-derived from `state` (#1493 P9).
        assertEquals("Waiting for approval…", out.statusLabel)
        assertEquals("warning", out.statusTone)
    }

    @Test
    fun selectsByKeyAndSchema() {
        val env = TypedProjectionEnvelope(
            key = TypedSignerStateDecoder.KEY,
            schemaId = TypedSignerStateDecoder.SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedSignerStateDecoder.FILE_IDENTIFIER,
            payload = readyBuffer(),
        )
        assertNull(TypedSignerStateDecoder.decode(listOf(env.copy(key = "other"))))
        assertNull(TypedSignerStateDecoder.decode(listOf(env.copy(schemaId = "other"))))
        assertEquals("ready", requireNotNull(TypedSignerStateDecoder.decode(listOf(env))).state)
    }

    @Test
    fun failedStateDerivesErrorLabelAndTone() {
        val out = requireNotNull(TypedSignerStateDecoder.decode(failedBuffer()))
        assertEquals("failed", out.state)
        assertTrue(out.isFailed)
        // Shell-derived from `state` (#1493 P9).
        assertEquals("Connection failed", out.statusLabel)
        assertEquals("error", out.statusTone)
    }

    // ── builders ───────────────────────────────────────────────────────────────

    private fun readyBuffer(): ByteArray {
        val b = FlatBufferBuilder(256)
        val signerKind = b.createString("nip46")
        val state = b.createString("ready")
        SignerState.startSignerState(b)
        SignerState.addSignerKind(b, signerKind)
        SignerState.addState(b, state)
        SignerState.addIsReady(b, true)
        val off = SignerState.endSignerState(b)
        SignerState.finishSignerStateBuffer(b, off)
        return b.sizedByteArray()
    }

    private fun awaitingBuffer(): ByteArray {
        val b = FlatBufferBuilder(256)
        val signerKind = b.createString("nip55")
        val state = b.createString("awaiting_approval")
        val reason = b.createString("approve in Amber")
        SignerState.startSignerState(b)
        SignerState.addSignerKind(b, signerKind)
        SignerState.addState(b, state)
        SignerState.addHasReason(b, true)
        SignerState.addReason(b, reason)
        SignerState.addIsAwaitingApproval(b, true)
        val off = SignerState.endSignerState(b)
        SignerState.finishSignerStateBuffer(b, off)
        return b.sizedByteArray()
    }

    private fun failedBuffer(): ByteArray {
        val b = FlatBufferBuilder(256)
        val signerKind = b.createString("nip46")
        val state = b.createString("failed")
        SignerState.startSignerState(b)
        SignerState.addSignerKind(b, signerKind)
        SignerState.addState(b, state)
        SignerState.addIsFailed(b, true)
        val off = SignerState.endSignerState(b)
        SignerState.finishSignerStateBuffer(b, off)
        return b.sizedByteArray()
    }
}
