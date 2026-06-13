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
import org.nmp.android.model.RelayRoleOption
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
        // PR-B (#991/#979): the generic `payload:Value` slot is no longer emitted.
        // The decode spine is rebuilt entirely from:
        //   - Tier-3 `SnapshotFrame` envelope fields: rev, running, metrics,
        //     relay_statuses, last_error_toast (ADR-0044)
        //   - Typed projection sidecars for every named projection key (ADR-0037)
        // iOS `KernelUpdateFrameDecoder.swift` follows the same approach and was
        // unaffected by PR-B because it never read `payload` (#1084).
        val typedProjections = extractTypedProjections(snapshot)
        val update = decodeKernelUpdate(snapshot, typedProjections) ?: return null
        return KernelDecodedUpdateFrame.Snapshot(update, typedProjections)
    }

    private fun decodeKernelUpdate(
        snapshot: SnapshotFrame,
        typedProjections: List<TypedProjectionEnvelope>,
    ): KernelUpdate? {
        return try {
            KernelUpdate(
                // rev, running, metrics, relayStatuses, lastErrorToast all come
                // from the Tier-3 SnapshotFrame envelope (ADR-0044). The generic
                // `payload:Value` root map is no longer present (PR-B #991/#979).
                rev = snapshot.rev.toLong(),
                running = snapshot.running,
                relayUrl = "",  // legacy field — no Tier-3 equivalent; never used by UI
                metrics = decodeMetricsFromTier3(snapshot),
                relayStatuses = decodeRelayStatusesFromTier3(snapshot),
                lastErrorToast = snapshot.lastErrorToast,
                projections = decodeProjections(typedProjections),
            )
        } catch (e: Exception) {
            Log.e(TAG, "KernelUpdate reconstruction failed: ${e.message}")
            null
        }
    }

    private fun decodeMetricsFromTier3(snapshot: SnapshotFrame): KernelMetricsLite? {
        val m = snapshot.metrics ?: return null
        return KernelMetricsLite(
            storedEvents = m.storedEvents.toLong(),
            visibleItems = m.visibleItems.toLong(),
            eventsRx = m.eventsRx.toLong(),
            updateSequence = m.updateSequence.toLong(),
        )
    }

    private fun decodeRelayStatusesFromTier3(snapshot: SnapshotFrame): List<RelayStatus> {
        val count = snapshot.relayStatusesLength
        if (count == 0) return emptyList()
        val result = ArrayList<RelayStatus>(count)
        for (i in 0 until count) {
            val rs = snapshot.relayStatuses(i) ?: continue
            result.add(
                RelayStatus(
                    role = rs.role ?: "",
                    relayUrl = rs.relayUrl ?: "",
                    connection = rs.connection ?: "",
                    auth = rs.auth ?: "",
                    activeWireSubscriptions = rs.activeWireSubscriptions.toInt(),
                    reconnectCount = rs.reconnectCount.toLong(),
                )
            )
        }
        return result
    }

    private fun decodeProjections(
        typedProjections: List<TypedProjectionEnvelope>,
    ): SnapshotProjections {
        // PR-B (#991/#979): the generic `payload:Value` projections sub-map is no
        // longer present on the wire. Every projection is typed-first via its
        // FlatBuffers sidecar. The `?: emptyList()` / `?: emptyMap()` chains below
        // handle the case where a typed sidecar is absent (ADR-0037 Commitment 4:
        // the generic fallback path is retained structurally but its Value source
        // is gone; the effective behaviour is: absent typed sidecar → empty/null).
        val typedWallet = TypedWalletDecoder.decode(typedProjections)
        val typedActiveAccount = TypedAccountsDecoder.decodeActiveAccount(typedProjections)
        return SnapshotProjections(
            activeAccount = typedActiveAccount?.pubkey,
            accounts = TypedAccountsDecoder.decodeAccounts(typedProjections) ?: emptyList(),
            claimedProfiles = TypedProfilesDecoder.decodeClaimed(typedProjections) ?: emptyMap(),
            mentionProfiles = emptyMap(),
            resolvedProfiles = TypedProfilesDecoder.decodeResolved(typedProjections) ?: emptyMap(),
            flatFeeds = TypedHomeFeedDecoder.decodeFlatFeeds(typedProjections),
            dmInbox = TypedDmInboxDecoder.decode(typedProjections),
            walletStatus = typedWallet?.status,
            walletBalance = typedWallet?.balanceDisplay,
            // ADR-0032 / #623: propagate the pre-computed label and tone from the
            // typed NIP-47 decoder so WalletScreen never branches on raw strings.
            walletLabel = typedWallet?.statusLabel,
            walletTone = typedWallet?.statusTone,
            relayRoleOptions = TypedRelayRoleOptionsDecoder.decode(typedProjections) ?: emptyList(),
            marmotSnapshot = TypedMarmotDecoder.decodeSnapshot(typedProjections),
            marmotMessages = TypedMarmotDecoder.decodeMessages(typedProjections) ?: emptyMap(),
            // #1099 / ADR-0048: the four typed sidecars iOS already decoded but
            // Android never wired — the signer badge (signer_state) and Marmot
            // dialog dismissal (action_lifecycle / action_results) were broken
            // because these arrived only as typed buffers post-PR-B. Each falls
            // back to null/empty when its sidecar is absent (ADR-0037).
            signerState = TypedSignerStateDecoder.decode(typedProjections),
            actionLifecycle = TypedActionLifecycleDecoder.decode(typedProjections),
            actionStages = TypedActionStagesDecoder.decode(typedProjections) ?: emptyMap(),
            actionResults = TypedActionResultsDecoder.decode(typedProjections) ?: emptyList(),
            relayDiagnostics = TypedRelayDiagnosticsDecoder.decode(typedProjections),
        )
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Typed projection sidecar extraction (ADR-0037)
    // ─────────────────────────────────────────────────────────────────────────

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

    // ─────────────────────────────────────────────────────────────────────────
    // Generic Value-map helpers — retained for legacy path compatibility.
    // These are no longer used by the main decode spine (PR-B #991/#979) but
    // remain available for any test or future projection that still needs to
    // walk a generic Value tree.
    // ─────────────────────────────────────────────────────────────────────────

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
