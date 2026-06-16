package org.nmp.gallery.bridge

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.ProfileCard
import nmp.kernel.ResolvedProfileEntry
import nmp.kernel.ResolvedProfilesSnapshot
import nmp.kernel.SignerState
import nmp.transport.FrameKind
import nmp.transport.PanicFrame
import nmp.transport.ProjectionPresenceState
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

@OptIn(ExperimentalUnsignedTypes::class)
class NmpUpdateFrameDecoderTest {

    @Test
    fun missing_identifier_throws_invalid_flatbuffer() {
        val bytes = ByteArray(32)
        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(bytes) { null }
        }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidFlatbuffer, ex.kind)
    }

    @Test
    fun panic_frame_throws_unexpected_panic() {
        val builder = FlatBufferBuilder()
        val msg = builder.createString("actor exploded")
        val panic = PanicFrame.createPanicFrame(builder, msg)
        val root = UpdateFrame.createUpdateFrame(builder, FrameKind.Panic, 0, panic)
        UpdateFrame.finishUpdateFrameBuffer(builder, root)

        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(builder.sizedByteArray()) { null }
        }
        assertEquals(UpdateFrameDecodeErrorKind.UnexpectedPanicFrame, ex.kind)
        assertTrue(ex.message!!.contains("actor exploded"))
    }

    @Test
    fun snapshot_schema_version_mismatch_throws() {
        val bytes = buildSnapshotFrame(schemaVersion = 99u)

        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(bytes) { null }
        }
        assertEquals(UpdateFrameDecodeErrorKind.SchemaVersionMismatch, ex.kind)
    }

    @Test
    fun snapshot_without_sidecars_decodes_tier3_running_field() {
        val decoded = NmpUpdateFrameDecoder.decodeSnapshot(
            buildSnapshotFrame(running = true),
        ) { null }

        assertTrue(decoded.running)
        assertTrue(decoded.resolvedProfiles.isEmpty())
        assertTrue(decoded.claimedEvents.isEmpty())
        assertNull(decoded.signerState)
    }

    @Test
    fun typed_sidecars_decode_resolved_profiles_and_signer_state() {
        val pubkey = "pubkey-a"
        val bytes = buildSnapshotFrame(
            running = true,
            projections = listOf(
                TypedProjectionInput(
                    key = "resolved_profiles",
                    schemaId = "resolved_profiles",
                    fileIdentifier = "KRPR",
                    payload = resolvedProfilesSidecar(pubkey),
                ),
                TypedProjectionInput(
                    key = "signer_state",
                    schemaId = "signer_state",
                    fileIdentifier = "KSST",
                    payload = signerStateSidecar(),
                ),
            ),
        )

        val decoded = NmpUpdateFrameDecoder.decodeSnapshot(bytes) { key ->
            if (key == pubkey) "npub1typed" else null
        }

        assertTrue(decoded.running)
        val profile = decoded.resolvedProfiles.getValue(pubkey)
        assertEquals(pubkey, profile.pubkey)
        assertEquals("Ada", profile.displayName)
        assertEquals("https://example.test/avatar.png", profile.pictureUrl)
        assertEquals("ada@example.test", profile.nip05)
        assertEquals("npub1typed", profile.npub)
        assertEquals("npub1typed", profile.npubShort)

        val signer = decoded.signerState!!
        assertEquals("nip55", signer.signerKind)
        assertEquals("awaiting_approval", signer.state)
        assertEquals("approve request", signer.reason)
        assertTrue(signer.isAwaitingApproval)
    }

    private data class TypedProjectionInput(
        val key: String,
        val schemaId: String,
        val fileIdentifier: String,
        val payload: ByteArray,
        val schemaVersion: UInt = 1u,
        val state: UByte = ProjectionPresenceState.Changed,
    )

    private fun buildSnapshotFrame(
        schemaVersion: UInt = 1u,
        running: Boolean = false,
        projections: List<TypedProjectionInput> = emptyList(),
    ): ByteArray {
        val builder = FlatBufferBuilder()
        val projectionOffsets = projections.map { projection ->
            val key = builder.createString(projection.key)
            val schemaId = builder.createString(projection.schemaId)
            val fileIdentifier = builder.createString(projection.fileIdentifier)
            val payloadBytes = TypedPayload.createPayloadVector(
                builder,
                projection.payload.asUByteArray(),
            )
            val payload = TypedPayload.createTypedPayload(
                builder,
                schemaId,
                projection.schemaVersion,
                fileIdentifier,
                payloadBytes,
            )
            TypedProjection.createTypedProjection(
                builder,
                key,
                payload,
                1u,
                projection.state,
            )
        }.toIntArray()
        val projectionVector = if (projectionOffsets.isEmpty()) {
            0
        } else {
            SnapshotFrame.createTypedProjectionsVector(builder, projectionOffsets)
        }
        val snapshot = SnapshotFrame.createSnapshotFrame(
            builder,
            schemaVersion,
            projectionVector,
            42u,
            1u,
            123u,
            0,
            running,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            null,
            0,
            7u,
            9u,
        )
        val root = UpdateFrame.createUpdateFrame(builder, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(builder, root)
        return builder.sizedByteArray()
    }

    private fun resolvedProfilesSidecar(pubkey: String): ByteArray {
        val builder = FlatBufferBuilder()
        val pubkeyOffset = builder.createString(pubkey)
        val displayName = builder.createString("Ada")
        val pictureUrl = builder.createString("https://example.test/avatar.png")
        val nip05 = builder.createString("ada@example.test")
        val about = builder.createString("Builder")
        val card = ProfileCard.createProfileCard(
            builder,
            pubkeyOffset,
            true,
            displayName,
            true,
            pictureUrl,
            nip05,
            about,
            false,
            0,
        )
        val key = builder.createString(pubkey)
        val entry = ResolvedProfileEntry.createResolvedProfileEntry(builder, key, card)
        val entries = ResolvedProfilesSnapshot.createEntriesVector(builder, intArrayOf(entry))
        val snapshot = ResolvedProfilesSnapshot.createResolvedProfilesSnapshot(builder, entries)
        ResolvedProfilesSnapshot.finishResolvedProfilesSnapshotBuffer(builder, snapshot)
        return builder.sizedByteArray()
    }

    private fun signerStateSidecar(): ByteArray {
        val builder = FlatBufferBuilder()
        val kind = builder.createString("nip55")
        val state = builder.createString("awaiting_approval")
        val reason = builder.createString("approve request")
        val statusLabel = builder.createString("Waiting")
        val statusTone = builder.createString("warning")
        val snapshot = SignerState.createSignerState(
            builder,
            kind,
            state,
            true,
            reason,
            false,
            true,
            false,
            false,
            false,
            statusLabel,
            statusTone,
        )
        SignerState.finishSignerStateBuffer(builder, snapshot)
        return builder.sizedByteArray()
    }

    private fun expectDecodeException(block: () -> Unit): UpdateFrameDecodeException {
        try {
            block()
        } catch (e: UpdateFrameDecodeException) {
            return e
        }
        fail("expected UpdateFrameDecodeException")
        error("unreachable")
    }
}
