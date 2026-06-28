package org.nmp.android

import android.util.Log
import nmp.marmot.KeyPackageStatus as FbKeyPackageStatus
import nmp.marmot.MarmotGroupMessages as FbMarmotGroupMessages
import nmp.marmot.MarmotGroupRow as FbMarmotGroupRow
import nmp.marmot.MarmotMessageRow as FbMarmotMessageRow
import nmp.marmot.MarmotMessages as FbMarmotMessages
import nmp.marmot.LastOpError as FbLastOpError
import nmp.marmot.MarmotSnapshot as FbMarmotSnapshot
import nmp.marmot.PendingOpRow as FbPendingOpRow
import nmp.marmot.PendingWelcomeRow as FbPendingWelcomeRow
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotKeyPackage
import org.nmp.android.model.MarmotLastOpError
import org.nmp.android.model.MarmotMessage
import org.nmp.android.model.MarmotPendingOp
import org.nmp.android.model.MarmotPendingWelcome
import org.nmp.android.model.MarmotSnapshot
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedMarmotDecoder"

/**
 * Typed-first decoder for the Marmot (MLS-over-Nostr) push projections
 * `nmp.marmot.snapshot` (`NMMS` / `MarmotSnapshot`) and `nmp.marmot.messages`
 * (`NMMG` / `MarmotMessages`) — the Android peer of iOS `TypedMarmotSnapshotDecoder`
 * / `TypedMarmotMessagesDecoder` (`TypedProjectionDecoders.generated.swift`) +
 * `TypedProjectionGlue.marmotSnapshot` / `marmotMessages`.
 *
 * Verbatim mirror of the Rust DTOs in `crates/nmp-marmot/src/projection/payload.rs`
 * (V-107 / ADR-0039). The `has_*` companion bools reproduce the JSON
 * null-when-absent semantics (ADR-0032). The messages wire flattens
 * `group_id_hex -> [message]` to a vector, rebuilt into the Android
 * `Map<String, List<MarmotMessage>>` domain shape.
 *
 * Both wires carry `schema_version`; an unknown version fails closed to `null`
 * rather than mis-reading a future layout.
 *
 * Each `decode*` returns `null` when its sidecar is absent / wrong schema /
 * unverifiable, so the typed-only host keeps the Marmot projection absent. Fail
 * closed (D1/D6) on a malformed buffer.
 */
object TypedMarmotDecoder {

    const val SNAPSHOT_KEY = "nmp.marmot.snapshot"
    const val SNAPSHOT_SCHEMA_ID = "nmp.marmot.snapshot"
    const val SNAPSHOT_FILE_IDENTIFIER = "NMMS"

    const val MESSAGES_KEY = "nmp.marmot.messages"
    const val MESSAGES_SCHEMA_ID = "nmp.marmot.messages"
    const val MESSAGES_FILE_IDENTIFIER = "NMMG"

    // Snapshot (NMMS): v1 = original shape; v2 adds pending_ops + last_op_error;
    // v3 removes age_display/subtitle/action_label, adds is_registered on KeyPackageStatus;
    // v4 removes display_name/initials/invites_chip_label/display_label (shells now own presentation);
    // v5 replaces keyring_unavailable:bool with init_error_kind/init_error_detail (#1651).
    private val SUPPORTED_SNAPSHOT_SCHEMA_VERSIONS: Set<UInt> = setOf(1u, 2u, 3u, 4u, 5u)

    // Messages (NMMG): unchanged at v1.
    private const val SUPPORTED_MESSAGES_SCHEMA_VERSION: UInt = 1u

    // ── snapshot ─────────────────────────────────────────────────────────────

    fun decodeSnapshot(projections: List<TypedProjectionEnvelope>): MarmotSnapshot? {
        val payload = selectPayload(projections, SNAPSHOT_KEY, SNAPSHOT_SCHEMA_ID) ?: return null
        return decodeSnapshot(payload)
    }

    /** Decode a raw `NMMS` buffer; `null` on any parse failure. */
    fun decodeSnapshot(bytes: ByteArray): MarmotSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbMarmotSnapshot.MarmotSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "NMMS file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snap = FbMarmotSnapshot.getRootAsMarmotSnapshot(bb)
            if (snap.schemaVersion !in SUPPORTED_SNAPSHOT_SCHEMA_VERSIONS) return null
            val groups = buildList {
                for (i in 0 until snap.groupsLength) {
                    val g = snap.groups(i) ?: continue
                    add(mapGroup(g))
                }
            }
            val welcomes = buildList {
                for (i in 0 until snap.pendingWelcomesLength) {
                    val w = snap.pendingWelcomes(i) ?: continue
                    add(mapWelcome(w))
                }
            }
            val cachedKp = buildList {
                for (i in 0 until snap.cachedKpPubkeysLength) {
                    add(snap.cachedKpPubkeys(i) ?: continue)
                }
            }
            // v2 additive fields. Empty vector / absent table on v1 buffers.
            val pendingOps = buildList {
                for (i in 0 until snap.pendingOpsLength) {
                    val op = snap.pendingOps(i) ?: continue
                    add(mapPendingOp(op))
                }
            }
            MarmotSnapshot(
                groups = groups,
                pendingWelcomes = welcomes,
                keyPackage = snap.keyPackage?.let { mapKeyPackage(it) } ?: MarmotKeyPackage(),
                cachedKpPubkeys = cachedKp,
                isRegistered = snap.isRegistered,
                orphanedCommitCount = snap.orphanedCommitCount.toInt(),
                initErrorKind = snap.initErrorKind ?: "",
                initErrorDetail = snap.initErrorDetail ?: "",
                pendingOps = pendingOps,
                lastOpError = snap.lastOpError?.let { mapLastOpError(it) },
            )
        } catch (e: Exception) {
            Log.e(TAG, "NMMS decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private fun mapGroup(g: FbMarmotGroupRow): MarmotGroup {
        val members = buildList {
            for (i in 0 until g.membersLength) {
                add(g.members(i) ?: continue)
            }
        }
        return MarmotGroup(
            idHex = g.idHex ?: "",
            name = g.name ?: "",
            members = members,
            memberCount = g.memberCount.toInt(),
            unreadCount = if (g.hasUnreadCount) g.unreadCount.toInt() else null,
            lastMsgAt = if (g.hasLastMsgAt) g.lastMsgAt.toLong() else null,
        )
    }

    private fun mapWelcome(w: FbPendingWelcomeRow): MarmotPendingWelcome = MarmotPendingWelcome(
        idHex = w.idHex ?: "",
        groupName = w.groupName ?: "",
        inviterNpub = w.inviterNpub ?: "",
    )

    private fun mapKeyPackage(kp: FbKeyPackageStatus): MarmotKeyPackage = MarmotKeyPackage(
        published = kp.published,
        dTag = if (kp.hasDTag) kp.dTag else null,
        ageSecs = if (kp.hasAgeSecs) kp.ageSecs.toLong() else null,
        stale = kp.stale,
        isRegistered = kp.isRegistered,
    )

    private fun mapPendingOp(op: FbPendingOpRow): MarmotPendingOp = MarmotPendingOp(
        correlationId = op.correlationId ?: "",
        opTag = op.opTag ?: "",
        missingCount = op.missingCount.toInt(),
        ageSecs = op.ageSecs.toLong(),
    )

    private fun mapLastOpError(err: FbLastOpError): MarmotLastOpError = MarmotLastOpError(
        op = err.op ?: "",
        reason = err.reason ?: "",
        atSecs = err.atSecs.toLong(),
        correlationId = err.correlationId ?: "",
    )

    // ── messages ─────────────────────────────────────────────────────────────

    fun decodeMessages(projections: List<TypedProjectionEnvelope>): Map<String, List<MarmotMessage>>? {
        val payload = selectPayload(projections, MESSAGES_KEY, MESSAGES_SCHEMA_ID) ?: return null
        return decodeMessages(payload)
    }

    /** Decode a raw `NMMG` buffer; `null` on any parse failure. */
    fun decodeMessages(bytes: ByteArray): Map<String, List<MarmotMessage>>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbMarmotMessages.MarmotMessagesBufferHasIdentifier(bb)) {
                Log.e(TAG, "NMMG file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val msgs = FbMarmotMessages.getRootAsMarmotMessages(bb)
            if (msgs.schemaVersion != SUPPORTED_MESSAGES_SCHEMA_VERSION) return null
            val result = LinkedHashMap<String, List<MarmotMessage>>(msgs.groupsLength * 2)
            for (i in 0 until msgs.groupsLength) {
                val group = msgs.groups(i) ?: continue
                val key = group.groupIdHex ?: continue
                result[key] = mapGroupMessages(group)
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "NMMG decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private fun mapGroupMessages(group: FbMarmotGroupMessages): List<MarmotMessage> = buildList {
        for (i in 0 until group.messagesLength) {
            val m = group.messages(i) ?: continue
            add(mapMessage(m))
        }
    }

    private fun mapMessage(m: FbMarmotMessageRow): MarmotMessage = MarmotMessage(
        id = m.id ?: "",
        senderPubkeyHex = m.senderPubkeyHex ?: "",
        content = m.content ?: "",
        createdAt = m.createdAt.toLong(),
        epoch = if (m.hasEpoch) m.epoch.toLong() else null,
    )

    // ── shared ───────────────────────────────────────────────────────────────

    private fun selectPayload(
        projections: List<TypedProjectionEnvelope>,
        key: String,
        schemaId: String,
    ): ByteArray? {
        val projection = projections.firstOrNull {
            it.key == key && it.schemaId == schemaId
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return projection.payload
    }
}
