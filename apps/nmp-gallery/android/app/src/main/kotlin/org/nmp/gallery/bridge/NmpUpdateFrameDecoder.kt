package org.nmp.gallery.bridge

import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import nmp.kernel.ProfileCard as FbProfileCard
import nmp.kernel.ResolvedProfilesSnapshot
import nmp.kernel.SignerState as FbSignerState
import nmp.transport.FrameKind
import nmp.transport.ProjectionPresenceState
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import org.nmp.gallery.registry.LoginBlockSignerState
import org.nmp.gallery.registry.ProfileWire

internal const val SCHEMA_VERSION_EXPECTED: UInt = 1u

internal sealed class UpdateFrameDecodeErrorKind {
    object InvalidFlatbuffer : UpdateFrameDecodeErrorKind()
    object MissingSnapshotPayload : UpdateFrameDecodeErrorKind()
    object MissingPanicPayload : UpdateFrameDecodeErrorKind()
    object UnexpectedPanicFrame : UpdateFrameDecodeErrorKind()
    object SchemaVersionMismatch : UpdateFrameDecodeErrorKind()
}

internal class UpdateFrameDecodeException(
    val kind: UpdateFrameDecodeErrorKind,
    message: String,
) : RuntimeException("${kind::class.simpleName}: $message")

internal data class GalleryDecodedSnapshot(
    val running: Boolean,
    val resolvedProfiles: Map<String, ProfileWire>,
    val claimedEvents: Map<String, ClaimedEventWire>,
    val signerState: LoginBlockSignerState?,
)

@OptIn(ExperimentalUnsignedTypes::class)
internal data class TypedProjectionEnvelope(
    val key: String,
    val schemaId: String,
    val schemaVersion: UInt,
    val fileIdentifier: String,
    val payload: ByteArray,
    val state: UByte,
)

@OptIn(ExperimentalUnsignedTypes::class)
internal object NmpUpdateFrameDecoder {
    private const val TAG = "NmpUpdateFrameDecoder"

    fun decodeSnapshot(
        bytes: ByteArray,
        npubFor: (pubkey: String) -> String?,
    ): GalleryDecodedSnapshot {
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        if (!UpdateFrame.UpdateFrameBufferHasIdentifier(buffer)) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                "missing NMPU file identifier",
            )
        }
        val frame = try {
            UpdateFrame.getRootAsUpdateFrame(buffer)
        } catch (e: Throwable) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                e.message ?: e.javaClass.simpleName,
            )
        }
        when (frame.kind) {
            FrameKind.Snapshot -> {
                // proceed below
            }
            FrameKind.Panic -> {
                val panic = frame.panic ?: throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.MissingPanicPayload,
                    "panic frame missing payload",
                )
                val msg = try {
                    panic.msg
                } catch (e: AssertionError) {
                    throw UpdateFrameDecodeException(
                        UpdateFrameDecodeErrorKind.MissingPanicPayload,
                        "panic frame missing msg",
                    )
                }
                throw UpdateFrameDecodeException(
                    UpdateFrameDecodeErrorKind.UnexpectedPanicFrame,
                    msg,
                )
            }
            else -> throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.InvalidFlatbuffer,
                "unknown frame kind ${frame.kind}",
            )
        }
        val snapshot = frame.snapshot ?: throw UpdateFrameDecodeException(
            UpdateFrameDecodeErrorKind.MissingSnapshotPayload,
            "snapshot frame missing snapshot",
        )
        if (snapshot.schemaVersion != SCHEMA_VERSION_EXPECTED) {
            throw UpdateFrameDecodeException(
                UpdateFrameDecodeErrorKind.SchemaVersionMismatch,
                "frame schema_version=${snapshot.schemaVersion} host=$SCHEMA_VERSION_EXPECTED",
            )
        }
        val projections = extractTypedProjections(snapshot)
        return GalleryDecodedSnapshot(
            running = snapshot.running,
            resolvedProfiles = decodeResolvedProfiles(projections, npubFor),
            claimedEvents = decodeClaimedEvents(projections),
            signerState = decodeSignerState(projections),
        )
    }

    private fun extractTypedProjections(snapshot: SnapshotFrame): List<TypedProjectionEnvelope> {
        val count = snapshot.typedProjectionsLength
        if (count == 0) return emptyList()
        val result = ArrayList<TypedProjectionEnvelope>(count)
        for (i in 0 until count) {
            val projection: TypedProjection = snapshot.typedProjections(i) ?: continue
            val key = projection.key ?: continue
            val wireState = projection.state
            if (wireState == ProjectionPresenceState.Cleared) {
                result.add(TypedProjectionEnvelope(key, "", 0u, "", ByteArray(0), wireState))
                continue
            }
            val typed: TypedPayload = projection.payload ?: continue
            val schemaId = typed.schemaId ?: continue
            val buf = typed.payloadAsByteBuffer
            val payloadBytes = ByteArray(buf.remaining())
            buf.get(payloadBytes)
            result.add(
                TypedProjectionEnvelope(
                    key = key,
                    schemaId = schemaId,
                    schemaVersion = typed.schemaVersion,
                    fileIdentifier = typed.fileIdentifier ?: "",
                    payload = payloadBytes,
                    state = wireState,
                )
            )
        }
        return result
    }

    private fun selectPayload(
        projections: List<TypedProjectionEnvelope>,
        key: String,
        schemaId: String,
        fileIdentifier: String,
    ): ByteArray? {
        val projection = projections.firstOrNull {
            it.key == key &&
                it.schemaId == schemaId &&
                it.schemaVersion == SCHEMA_VERSION_EXPECTED &&
                it.fileIdentifier == fileIdentifier &&
                it.state != ProjectionPresenceState.Cleared
        } ?: return null
        return projection.payload.takeIf { it.isNotEmpty() }
    }

    private fun decodeResolvedProfiles(
        projections: List<TypedProjectionEnvelope>,
        npubFor: (pubkey: String) -> String?,
    ): Map<String, ProfileWire> {
        val payload = selectPayload(projections, "resolved_profiles", "resolved_profiles", "KRPR")
            ?: return emptyMap()
        return try {
            val bb = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
            if (!ResolvedProfilesSnapshot.ResolvedProfilesSnapshotBufferHasIdentifier(bb)) {
                Log.w(TAG, "drop KRPR sidecar: missing file identifier")
                return emptyMap()
            }
            val snapshot = ResolvedProfilesSnapshot.getRootAsResolvedProfilesSnapshot(bb)
            buildMap {
                for (i in 0 until snapshot.entriesLength) {
                    val entry = snapshot.entries(i) ?: continue
                    val key = entry.key ?: continue
                    val card = entry.value ?: continue
                    put(key, profileWire(card, key, npubFor))
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "drop KRPR sidecar: ${e.message}")
            emptyMap()
        }
    }

    private fun profileWire(
        card: FbProfileCard,
        fallbackPubkey: String,
        npubFor: (pubkey: String) -> String?,
    ): ProfileWire {
        val pubkey = card.pubkey ?: fallbackPubkey
        val npub = npubFor(pubkey).orEmpty()
        return ProfileWire(
            pubkey = pubkey,
            displayName = if (card.hasDisplayName) card.displayName?.takeIf { it.isNotEmpty() } else null,
            about = card.about?.takeIf { it.isNotEmpty() },
            pictureUrl = if (card.hasPictureUrl) card.pictureUrl?.takeIf { it.isNotEmpty() } else null,
            nip05 = card.nip05?.takeIf { it.isNotEmpty() },
            npub = npub,
            npubShort = npub.takeIf { it.isNotEmpty() }?.let(::shortIdentifier)
                ?: shortIdentifier(pubkey),
        )
    }

    private fun decodeSignerState(
        projections: List<TypedProjectionEnvelope>,
    ): LoginBlockSignerState? {
        val payload = selectPayload(projections, "signer_state", "signer_state", "KSST")
            ?: return null
        return try {
            val bb = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbSignerState.SignerStateBufferHasIdentifier(bb)) {
                Log.w(TAG, "drop KSST sidecar: missing file identifier")
                return null
            }
            val signer = FbSignerState.getRootAsSignerState(bb)
            LoginBlockSignerState(
                signerKind = signer.signerKind ?: "",
                state = signer.state ?: "",
                reason = if (signer.hasReason) signer.reason else null,
                isReady = signer.isReady,
                isAwaitingApproval = signer.isAwaitingApproval,
                isReconnecting = signer.isReconnecting,
                isUnavailable = signer.isUnavailable,
                isFailed = signer.isFailed,
            )
        } catch (e: Exception) {
            Log.w(TAG, "drop KSST sidecar: ${e.message}")
            null
        }
    }

    private fun decodeClaimedEvents(
        projections: List<TypedProjectionEnvelope>,
    ): Map<String, ClaimedEventWire> {
        val payload = selectPayload(projections, "claimed_events", "claimed_events", "KCEV")
            ?: return emptyMap()
        return try {
            val reader = FbReader(payload)
            if (!reader.hasIdentifier("KCEV")) return emptyMap()
            val root = reader.rootTable()
            val entries = reader.tableVectorField(root, 0)
            buildMap {
                for (entry in entries) {
                    val key = reader.stringField(entry, 0) ?: continue
                    val event = reader.tableField(entry, 1)?.let { reader.claimedEvent(it, key) }
                        ?: continue
                    put(key, event)
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "drop KCEV sidecar: ${e.message}")
            emptyMap()
        }
    }

    private fun shortIdentifier(value: String): String =
        if (value.length > 16) value.take(8) + "…" + value.takeLast(8) else value

    private class FbReader(private val data: ByteArray) {
        fun hasIdentifier(id: String): Boolean =
            data.size >= 8 &&
                data[4] == id[0].code.toByte() &&
                data[5] == id[1].code.toByte() &&
                data[6] == id[2].code.toByte() &&
                data[7] == id[3].code.toByte()

        fun rootTable(): Int = u32(0).toInt()

        fun claimedEvent(table: Int, fallbackPrimaryId: String): ClaimedEventWire =
            ClaimedEventWire(
                id = stringField(table, 1).orEmpty(),
                authorPubkey = stringField(table, 2).orEmpty(),
                kind = u32Field(table, 7)?.toLong() ?: 0L,
                createdAt = u64Field(table, 8)?.toLong() ?: 0L,
                tags = tableVectorField(table, 9).map { tagRow ->
                    stringVectorField(tagRow, 0)
                },
                content = stringField(table, 10).orEmpty(),
                authorDisplayName = if (boolField(table, 3) == true) stringField(table, 4) else null,
                authorPictureUrl = if (boolField(table, 5) == true) stringField(table, 6) else null,
            ).let { event ->
                if (event.id.isNotEmpty()) event else event.copy(id = fallbackPrimaryId)
            }

        fun tableField(table: Int, index: Int): Int? =
            field(table, index)?.let(::indirect)

        fun tableVectorField(table: Int, index: Int): List<Int> {
            val field = field(table, index) ?: return emptyList()
            val vector = indirect(field)
            val count = u32(vector).toInt()
            return (0 until count).map { item ->
                indirect(vector + 4 + item * 4)
            }
        }

        fun stringVectorField(table: Int, index: Int): List<String> {
            val field = field(table, index) ?: return emptyList()
            val vector = indirect(field)
            val count = u32(vector).toInt()
            return (0 until count).mapNotNull { item ->
                string(indirect(vector + 4 + item * 4))
            }
        }

        fun stringField(table: Int, index: Int): String? =
            field(table, index)?.let { string(indirect(it)) }

        fun boolField(table: Int, index: Int): Boolean? =
            field(table, index)?.let { checked(it, 1); data[it] != 0.toByte() }

        fun u32Field(table: Int, index: Int): UInt? =
            field(table, index)?.let { u32(it) }

        fun u64Field(table: Int, index: Int): ULong? =
            field(table, index)?.let { u64(it) }

        private fun field(table: Int, index: Int): Int? {
            checked(table, 4)
            val vtable = table - i32(table)
            checked(vtable, 4)
            val length = u16(vtable).toInt()
            val entry = vtable + 4 + index * 2
            if (entry + 2 > vtable + length) return null
            val offset = u16(entry).toInt()
            return if (offset == 0) null else table + offset
        }

        private fun indirect(offset: Int): Int = offset + u32(offset).toInt()

        private fun string(offset: Int): String? {
            val length = u32(offset).toInt()
            val start = offset + 4
            checked(start, length)
            return String(data, start, length, Charsets.UTF_8)
        }

        private fun u16(offset: Int): UShort {
            checked(offset, 2)
            return ((data[offset].toInt() and 0xff) or
                ((data[offset + 1].toInt() and 0xff) shl 8)).toUShort()
        }

        private fun i32(offset: Int): Int = u32(offset).toInt()

        private fun u32(offset: Int): UInt {
            checked(offset, 4)
            return (((data[offset].toInt() and 0xff).toUInt()) or
                (((data[offset + 1].toInt() and 0xff).toUInt()) shl 8) or
                (((data[offset + 2].toInt() and 0xff).toUInt()) shl 16) or
                (((data[offset + 3].toInt() and 0xff).toUInt()) shl 24))
        }

        private fun u64(offset: Int): ULong {
            checked(offset, 8)
            var value = 0UL
            for (byte in 0 until 8) {
                value = value or ((data[offset + byte].toULong() and 0xffUL) shl (byte * 8))
            }
            return value
        }

        private fun checked(offset: Int, count: Int) {
            require(offset >= 0 && count >= 0 && offset + count <= data.size)
        }
    }
}
