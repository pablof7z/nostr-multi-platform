package org.nmp.android

import android.util.Log
import nmp.transport.FrameKind
import nmp.transport.Pair
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind
import org.nmp.android.model.AccountSummary
import org.nmp.android.model.DmConversation
import org.nmp.android.model.DmInboxSnapshot
import org.nmp.android.model.DmMessage
import org.nmp.android.model.KernelMetricsLite
import org.nmp.android.model.KernelUpdate
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotKeyPackage
import org.nmp.android.model.MarmotMessage
import org.nmp.android.model.MarmotPendingWelcome
import org.nmp.android.model.MarmotSnapshot
import org.nmp.android.model.ProfileCard
import org.nmp.android.model.RelayStatus
import org.nmp.android.model.SnapshotProjections
import org.nmp.android.model.TimelineItem
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "KernelUpdateFrameDecoder"

sealed interface KernelDecodedUpdateFrame {
    data class Snapshot(
        val update: KernelUpdate,
        val typedProjections: List<TypedProjectionEnvelope>,
    ) : KernelDecodedUpdateFrame

    data class Panic(val message: String) : KernelDecodedUpdateFrame
}

data class TypedProjectionEnvelope(
    val key: String,
    val schemaId: String,
    val schemaVersion: UInt,
    val fileIdentifier: String,
    val payload: ByteArray,
) {
    // ByteArray equality is structural; override to avoid identity comparison.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is TypedProjectionEnvelope) return false
        return key == other.key &&
            schemaId == other.schemaId &&
            schemaVersion == other.schemaVersion &&
            fileIdentifier == other.fileIdentifier &&
            payload.contentEquals(other.payload)
    }

    override fun hashCode(): Int {
        var result = key.hashCode()
        result = 31 * result + schemaId.hashCode()
        result = 31 * result + schemaVersion.hashCode()
        result = 31 * result + fileIdentifier.hashCode()
        result = 31 * result + payload.contentHashCode()
        return result
    }
}

object KernelUpdateFrameDecoder {

    fun decode(bytes: ByteArray): KernelDecodedUpdateFrame? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!UpdateFrame.UpdateFrameBufferHasIdentifier(bb)) {
                Log.e(TAG, "buffer missing NMPU identifier (${bytes.size} bytes)")
                return null
            }
            val frame = UpdateFrame.getRootAsUpdateFrame(bb)
            when (frame.kind) {
                FrameKind.Snapshot -> decodeSnapshot(frame, bytes.size)
                FrameKind.Panic -> {
                    val msg = frame.panic?.msg ?: "unknown panic"
                    Log.wtf(TAG, "NMP_ACTOR_PANIC: $msg bytes=${bytes.size}")
                    KernelDecodedUpdateFrame.Panic(msg)
                }
                else -> {
                    Log.e(TAG, "unknown FrameKind ${frame.kind}")
                    null
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private fun decodeSnapshot(frame: UpdateFrame, byteCount: Int): KernelDecodedUpdateFrame? {
        val snapshot = frame.snapshot ?: run {
            Log.e(TAG, "snapshot frame missing bytes=$byteCount")
            return null
        }
        val payload = snapshot.payload ?: run {
            Log.e(TAG, "snapshot.payload is null bytes=$byteCount")
            return null
        }
        val update = decodeKernelUpdate(payload) ?: return null
        val projections = extractTypedProjections(snapshot)
        return KernelDecodedUpdateFrame.Snapshot(update, projections)
    }

    private fun decodeKernelUpdate(root: Value): KernelUpdate? {
        if (root.kind != ValueKind.Map) {
            Log.e(TAG, "root value is not a map (kind=${root.kind})")
            return null
        }
        val map = buildValueMap(root)
        return try {
            KernelUpdate(
                rev = map["rev"]?.longOr(0L) ?: 0L,
                running = map["running"]?.boolOr(false) ?: false,
                relayUrl = map["relayUrl"]?.stringOr("") ?: "",
                legacyItems = map["items"]?.listOf { decodeTimelineItem(it) } ?: emptyList(),
                metrics = map["metrics"]?.let { decodeMetricsLite(it) },
                relayStatuses = map["relayStatuses"]?.listOf { decodeRelayStatus(it) } ?: emptyList(),
                lastErrorToast = map["lastErrorToast"]?.stringOrNull(),
                projections = map["projections"]?.let { decodeProjections(it) },
            )
        } catch (e: Exception) {
            Log.e(TAG, "KernelUpdate reconstruction failed: ${e.message}")
            null
        }
    }

    private fun decodeTimelineItem(v: Value): TimelineItem? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return TimelineItem(
            id = m["id"]?.stringOr("") ?: "",
            authorPubkey = m["authorPubkey"]?.stringOr("") ?: "",
            content = m["content"]?.stringOr("") ?: "",
            contentPreview = m["contentPreview"]?.stringOr("") ?: "",
            createdAt = m["createdAt"]?.longOr(0L) ?: 0L,
            relayCount = m["relayCount"]?.longOr(0L) ?: 0L,
        )
    }

    private fun decodeMetricsLite(v: Value): KernelMetricsLite? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return KernelMetricsLite(
            storedEvents = m["storedEvents"]?.longOr(0L) ?: 0L,
            visibleItems = m["visibleItems"]?.longOr(0L) ?: 0L,
            eventsRx = m["eventsRx"]?.longOr(0L) ?: 0L,
            updateSequence = m["updateSequence"]?.longOr(0L) ?: 0L,
        )
    }

    private fun decodeRelayStatus(v: Value): RelayStatus? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return RelayStatus(
            role = m["role"]?.stringOr("") ?: "",
            relayUrl = m["relayUrl"]?.stringOr("") ?: "",
            connection = m["connection"]?.stringOr("") ?: "",
            auth = m["auth"]?.stringOr("") ?: "",
            activeWireSubscriptions = m["activeWireSubscriptions"]?.intOr(0) ?: 0,
            reconnectCount = m["reconnectCount"]?.longOr(0L) ?: 0L,
        )
    }

    private fun decodeProjections(v: Value): SnapshotProjections? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return SnapshotProjections(
            activeAccount = m["activeAccount"]?.stringOrNull(),
            accounts = m["accounts"]?.listOf { decodeAccountSummary(it) } ?: emptyList(),
            claimedProfiles = m["claimedProfiles"]?.mapOf { decodeProfileCard(it) } ?: emptyMap(),
            mentionProfiles = m["mentionProfiles"]?.mapOf { decodeProfileCard(it) } ?: emptyMap(),
            resolvedProfiles = m["resolvedProfiles"]?.mapOf { decodeProfileCard(it) } ?: emptyMap(),
            flatFeeds = FlatFeedProjectionDecoder.decode(m),
            dmInbox = m["nmp.nip17.dmInbox"]?.let { decodeDmInboxSnapshot(it) },
            walletStatus = m["wallet"]?.let { decodeWalletStatusString(it) },
            walletBalance = m["wallet"]?.let { decodeWalletBalanceString(it) },
            marmotSnapshot = m["nmp.marmot.snapshot"]?.let { decodeMarmotSnapshot(it) },
            marmotMessages = m["nmp.marmot.messages"]
                ?.mapOf { groupMessages -> groupMessages.listOf { decodeMarmotMessage(it) } }
                ?: emptyMap(),
        )
    }

    private fun decodeMarmotSnapshot(v: Value): MarmotSnapshot? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return MarmotSnapshot(
            groups = m["groups"]?.listOf { decodeMarmotGroup(it) } ?: emptyList(),
            pendingWelcomes = m["pendingWelcomes"]?.listOf { decodeMarmotPendingWelcome(it) } ?: emptyList(),
            keyPackage = m["keyPackage"]?.let { decodeMarmotKeyPackage(it) } ?: MarmotKeyPackage(),
            cachedKpPubkeys = m["cachedKpPubkeys"]?.listOf { it.stringOrNull() } ?: emptyList(),
            invitesChipLabel = m["invitesChipLabel"]?.stringOrNull(),
            isRegistered = m["isRegistered"]?.boolOr(false) ?: false,
            orphanedCommitCount = m["orphanedCommitCount"]?.intOr(0) ?: 0,
            keyringUnavailable = m["keyringUnavailable"]?.boolOr(false) ?: false,
        )
    }

    private fun decodeMarmotGroup(v: Value): MarmotGroup? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return MarmotGroup(
            idHex = m["idHex"]?.stringOr("") ?: "",
            name = m["name"]?.stringOr("") ?: "",
            displayName = m["displayName"]?.stringOr("") ?: "",
            initials = m["initials"]?.stringOr("") ?: "",
            members = m["members"]?.listOf { it.stringOrNull() } ?: emptyList(),
            memberCount = m["memberCount"]?.intOr(0) ?: 0,
            unreadCount = m["unreadCount"]?.let { if (it.kind == ValueKind.Null) null else it.intOr(0) },
            lastMsgAt = m["lastMsgAt"]?.let { if (it.kind == ValueKind.Null) null else it.longOr(0L) },
        )
    }

    private fun decodeMarmotPendingWelcome(v: Value): MarmotPendingWelcome? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return MarmotPendingWelcome(
            idHex = m["idHex"]?.stringOr("") ?: "",
            groupName = m["groupName"]?.stringOr("") ?: "",
            displayName = m["displayName"]?.stringOr("") ?: "",
            inviterNpub = m["inviterNpub"]?.stringOr("") ?: "",
        )
    }

    private fun decodeMarmotKeyPackage(v: Value): MarmotKeyPackage? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return MarmotKeyPackage(
            published = m["published"]?.boolOr(false) ?: false,
            dTag = m["dTag"]?.stringOrNull(),
            ageSecs = m["ageSecs"]?.let { if (it.kind == ValueKind.Null) null else it.longOr(0L) },
            stale = m["stale"]?.boolOr(false) ?: false,
            ageDisplay = m["ageDisplay"]?.stringOrNull(),
            subtitle = m["subtitle"]?.stringOr("") ?: "",
            actionLabel = m["actionLabel"]?.stringOr("") ?: "",
        )
    }

    private fun decodeMarmotMessage(v: Value): MarmotMessage? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return MarmotMessage(
            id = m["id"]?.stringOr("") ?: "",
            senderPubkeyHex = m["senderPubkeyHex"]?.stringOr("") ?: "",
            content = m["content"]?.stringOr("") ?: "",
            createdAt = m["createdAt"]?.longOr(0L) ?: 0L,
            epoch = m["epoch"]?.let { if (it.kind == ValueKind.Null) null else it.longOr(0L) },
        )
    }

    private fun decodeProfileCard(v: Value): ProfileCard? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return ProfileCard(
            pubkey = m["pubkey"]?.stringOr("") ?: "",
            npub = m["npub"]?.stringOr("") ?: "",
            displayName = m["displayName"]?.stringOrNull(),
            pictureUrl = m["pictureUrl"]?.stringOrNull(),
            nip05 = m["nip05"]?.stringOr("") ?: "",
            about = m["about"]?.stringOr("") ?: "",
            hasProfile = m["hasProfile"]?.boolOr(false) ?: false,
            lnurl = m["lnurl"]?.stringOrNull(),
        )
    }

    private fun decodeDmInboxSnapshot(v: Value): DmInboxSnapshot? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return DmInboxSnapshot(
            conversations = m["conversations"]?.listOf { decodeDmConversation(it) } ?: emptyList(),
            remoteSignerUnsupported = m["remoteSignerUnsupported"]?.boolOr(false) ?: false,
        )
    }

    private fun decodeDmConversation(v: Value): DmConversation? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return DmConversation(
            peerPubkey = m["peerPubkey"]?.stringOr("") ?: "",
            messages = m["messages"]?.listOf { decodeDmMessage(it) } ?: emptyList(),
        )
    }

    private fun decodeDmMessage(v: Value): DmMessage? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return DmMessage(
            id = m["id"]?.stringOr("") ?: "",
            senderPubkey = m["senderPubkey"]?.stringOr("") ?: "",
            content = m["content"]?.stringOr("") ?: "",
            createdAt = m["createdAt"]?.longOr(0L) ?: 0L,
            replyTo = m["replyTo"]?.stringOrNull(),
            isOutgoing = m["isOutgoing"]?.boolOr(false) ?: false,
            sourceRelays = m["sourceRelays"]?.listOf { relay -> relay.stringOrNull() },
        )
    }

    private fun decodeWalletStatusString(v: Value): String? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return m["status"]?.stringOrNull()
    }

    private fun decodeWalletBalanceString(v: Value): String? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return m["balanceSatsDisplay"]?.stringOrNull()
    }

    private fun decodeAccountSummary(v: Value): AccountSummary? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return AccountSummary(
            id = m["id"]?.stringOr("") ?: "",
            npubShort = m["npubShort"]?.stringOr("") ?: "",
            displayName = m["displayName"]?.stringOr("") ?: "",
            status = m["status"]?.stringOr("") ?: "",
            signerLabel = m["signerLabel"]?.stringOr("") ?: "",
        )
    }

    private fun extractTypedProjections(snapshot: SnapshotFrame): List<TypedProjectionEnvelope> {
        val count = snapshot.typedProjectionsLength
        if (count == 0) return emptyList()
        val result = ArrayList<TypedProjectionEnvelope>(count)
        for (i in 0 until count) {
            val projection: TypedProjection = snapshot.typedProjections(i) ?: continue
            val key = projection.key ?: continue
            val typed: TypedPayload = projection.payload ?: continue
            val schemaId = typed.schemaId ?: continue
            val payloadBytes: ByteArray = typed.payloadAsByteBuffer?.let { buf ->
                val bytes = ByteArray(buf.remaining())
                buf.get(bytes)
                bytes
            } ?: ByteArray(0)
            result.add(
                TypedProjectionEnvelope(
                    key = key,
                    schemaId = schemaId,
                    schemaVersion = typed.schemaVersion,
                    fileIdentifier = typed.fileIdentifier ?: "",
                    payload = payloadBytes,
                )
            )
        }
        return result
    }

    private fun buildValueMap(v: Value): Map<String, Value> {
        val len = v.mapLength
        if (len == 0) return emptyMap()
        val result = HashMap<String, Value>(len * 2)
        for (i in 0 until len) {
            val pair: Pair = v.map(i) ?: continue
            val value: Value = pair.value ?: continue
            val key = pair.key
            result[convertFromSnakeCase(key)] = value
        }
        return result
    }

    private fun convertFromSnakeCase(key: String): String {
        if (!key.contains('_')) return key
        val leadingCount = key.indexOfFirst { it != '_' }.takeIf { it >= 0 } ?: return key
        val trailingCount = key.reversed().indexOfFirst { it != '_' }.takeIf { it >= 0 } ?: 0
        val start = leadingCount
        val end = key.length - trailingCount
        if (start >= end) return key
        val body = key.substring(start, end)
        val sb = StringBuilder(body.length)
        var capitalizeNext = false
        for (ch in body) {
            when {
                ch == '_' -> if (sb.isNotEmpty()) capitalizeNext = true
                capitalizeNext -> {
                    sb.append(ch.uppercaseChar())
                    capitalizeNext = false
                }
                else -> sb.append(ch)
            }
        }
        val leading = key.substring(0, start)
        val trailing = key.substring(end)
        return leading + sb.toString() + trailing
    }

    private fun Value.longOr(default: Long): Long = when (kind) {
        ValueKind.Int -> intValue
        ValueKind.UInt -> uintValue.toLong()
        else -> default
    }

    private fun Value.intOr(default: Int): Int = longOr(default.toLong()).toInt()

    private fun Value.boolOr(default: Boolean): Boolean = when (kind) {
        ValueKind.Bool -> boolValue
        else -> default
    }

    private fun Value.stringOr(default: String): String = when (kind) {
        ValueKind.String -> stringValue ?: default
        else -> default
    }

    private fun Value.stringOrNull(): String? = when (kind) {
        ValueKind.String -> stringValue
        ValueKind.Null -> null
        else -> null
    }

    private fun <T : Any> Value.listOf(decode: (Value) -> T?): List<T> {
        if (kind != ValueKind.List) return emptyList()
        val len = listLength
        if (len == 0) return emptyList()
        val result = ArrayList<T>(len)
        for (i in 0 until len) {
            val item: Value = list(i) ?: continue
            val decoded = decode(item) ?: continue
            result.add(decoded)
        }
        return result
    }

    private fun <T : Any> Value.mapOf(decode: (Value) -> T?): Map<String, T> {
        if (kind != ValueKind.Map) return emptyMap()
        val len = mapLength
        if (len == 0) return emptyMap()
        val result = HashMap<String, T>(len * 2)
        for (i in 0 until len) {
            val pair: nmp.transport.Pair = map(i) ?: continue
            val entryValue: Value = pair.value ?: continue
            val rawKey = pair.key
            val decoded = decode(entryValue) ?: continue
            result[rawKey] = decoded
        }
        return result
    }
}
