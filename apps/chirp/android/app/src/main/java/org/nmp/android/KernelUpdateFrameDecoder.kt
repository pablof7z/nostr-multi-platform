package org.nmp.android

import android.util.Log
import nmp.transport.FrameKind
import nmp.transport.ProjectionPresenceState
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import org.nmp.android.model.AccountSummary
import org.nmp.android.model.DmConversation
import org.nmp.android.model.DmInboxSnapshot
import org.nmp.android.model.DmMessage
import org.nmp.android.model.FollowListSnapshot
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
        // ADR-0055 R3-S4: session/epoch carried out of the single FlatBuffers decode
        // pass (read off the SnapshotFrame table next to schemaVersion — NOT a second
        // buffer parse). Fed to ProjectionMergeCache.merge() in KernelModel.
        val sessionId: ULong,
        val snapshotEpoch: ULong,
    ) : KernelDecodedUpdateFrame

    data class Panic(val message: String) : KernelDecodedUpdateFrame
}

@OptIn(ExperimentalUnsignedTypes::class)
data class TypedProjectionEnvelope(
    val key: String,
    val schemaId: String,
    val schemaVersion: UInt,
    val fileIdentifier: String,
    val payload: ByteArray,
    // ADR-0055 R3-S4: per-projection rev and wire state
    val projectionRev: ULong = 0UL,
    val state: UByte = ProjectionPresenceState.Changed,
) {
    // ByteArray equality is structural; override to avoid identity comparison.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is TypedProjectionEnvelope) return false
        return key == other.key &&
            schemaId == other.schemaId &&
            schemaVersion == other.schemaVersion &&
            fileIdentifier == other.fileIdentifier &&
            payload.contentEquals(other.payload) &&
            projectionRev == other.projectionRev &&
            state == other.state
    }

    override fun hashCode(): Int {
        var result = key.hashCode()
        result = 31 * result + schemaId.hashCode()
        result = 31 * result + schemaVersion.hashCode()
        result = 31 * result + fileIdentifier.hashCode()
        result = 31 * result + payload.contentHashCode()
        result = 31 * result + projectionRev.hashCode()
        result = 31 * result + state.hashCode()
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

    @OptIn(ExperimentalUnsignedTypes::class)
    private fun decodeSnapshot(frame: UpdateFrame, byteCount: Int): KernelDecodedUpdateFrame? {
        val snapshot = frame.snapshot ?: run {
            Log.e(TAG, "snapshot frame missing bytes=$byteCount")
            return null
        }
        // PR-B (#991/#979) and the follow-up schema cleanup removed the generic
        // `payload:Value` slot.
        // The decode spine is rebuilt entirely from:
        //   - Tier-3 `SnapshotFrame` envelope fields: rev, running, metrics,
        //     relay_statuses, last_error_toast (ADR-0044)
        //   - Typed projection sidecars for every named projection key (ADR-0037)
        // iOS `KernelUpdateFrameDecoder.swift` follows the same approach and was
        // unaffected by PR-B because it never read `payload` (#1084).
        //
        // ADR-0055 R3-S4: sessionId and snapshotEpoch are read off the SnapshotFrame
        // table in THIS single decode pass — we do NOT re-parse the buffer a second
        // time. They ride alongside schemaVersion on the same FlatBuffers table root.
        val sessionId: ULong = snapshot.sessionId
        val snapshotEpoch: ULong = snapshot.snapshotEpoch
        val typedProjections = extractTypedProjections(snapshot)
        val update = decodeKernelUpdate(snapshot, typedProjections) ?: return null
        return KernelDecodedUpdateFrame.Snapshot(
            update = update,
            typedProjections = typedProjections,
            sessionId = sessionId,
            snapshotEpoch = snapshotEpoch,
        )
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

    /**
     * ADR-0055 R3-S4: re-run projection decoding against a merged envelope set.
     * Called from [KernelModel.decodeUpdate] AFTER [ProjectionMergeCache.merge]
     * has reconstituted the full set (cached values for omitted keys, tombstones
     * removed for Cleared keys, fresh bytes for Changed keys).
     *
     * Exposed as `internal` so it is reachable from `KernelModel` (same module)
     * without widening to `public`.
     */
    internal fun decodeProjections(
        typedProjections: List<TypedProjectionEnvelope>,
    ): SnapshotProjections {
        // The generic `payload:Value` projections sub-map no longer exists on
        // the wire. Every host-visible projection arrives through a FlatBuffers
        // sidecar. The `?: emptyList()` / `?: emptyMap()` chains below handle
        // absent or failed typed sidecars with explicit empty/null defaults.
        val typedWallet = TypedWalletDecoder.decode(typedProjections)
        val typedActiveAccount = TypedAccountsDecoder.decodeActiveAccount(typedProjections)
        // ADR-0063 Lane H: claimed_profiles / mention_profiles / resolved_profiles deleted.
        // Profile data is now served via the refs.profile KPRF NRRD row-delta sidecar.
        return SnapshotProjections(
            activeAccount = typedActiveAccount?.pubkey,
            accounts = TypedAccountsDecoder.decodeAccounts(typedProjections) ?: emptyList(),
            flatFeeds = TypedHomeFeedDecoder.decodeFlatFeeds(typedProjections),
            dmInbox = TypedDmInboxDecoder.decode(typedProjections),
            walletStatus = typedWallet?.status,
            walletBalance = typedWallet?.balanceDisplay,
            // ADR-0032 / #623: propagate the pre-computed label and tone from the
            // typed NIP-47 decoder so WalletScreen never branches on raw strings.
            walletLabel = typedWallet?.statusLabel,
            walletTone = typedWallet?.statusTone,
            // Rust-computed connected flag bound verbatim (D7); the UI gates on
            // this instead of `walletTone != "inactive"`. Mirrors iOS.
            walletIsConnected = typedWallet?.isConnected,
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
            // #1283 / #1335 item 2: typed NEMB embed sidecar — decoded by
            // [TypedEmbedSidecarDecoder]. Empty map when the sidecar is absent
            // (ADR-0037 Commitment 4 fail-closed; no JNI claim/release yet — that
            // is the #984 follow-up; the decode + gallery-render path is complete).
            refEventEnvelopes = TypedEmbedSidecarDecoder.decode(typedProjections),
            followList = TypedFollowListDecoder.decode(typedProjections) ?: FollowListSnapshot(),
        )
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Typed projection sidecar extraction (ADR-0037)
    // ─────────────────────────────────────────────────────────────────────────

    @OptIn(ExperimentalUnsignedTypes::class)
    private fun extractTypedProjections(snapshot: SnapshotFrame): List<TypedProjectionEnvelope> {
        val count = snapshot.typedProjectionsLength
        if (count == 0) return emptyList()
        val result = ArrayList<TypedProjectionEnvelope>(count)
        for (i in 0 until count) {
            val projection: TypedProjection = snapshot.typedProjections(i) ?: continue
            val key = projection.key ?: continue
            // ADR-0055 R3-S4: read wire state from the FlatBuffers accessor (slot 10).
            // Cleared rows carry null payload by design — do NOT skip them.
            val wireState: UByte = projection.state
            // ADR-0055 R3-S4: read projectionRev from FlatBuffers (slot 8).
            val projectionRev: ULong = projection.projectionRev

            if (wireState == ProjectionPresenceState.Cleared) {
                // Cleared row: no payload expected. Emit a tombstone envelope so
                // ProjectionMergeCache.merge() can remove the key from its cache.
                result.add(
                    TypedProjectionEnvelope(
                        key = key,
                        schemaId = "",
                        schemaVersion = 0u,
                        fileIdentifier = "",
                        payload = ByteArray(0),
                        projectionRev = projectionRev,
                        state = wireState,
                    )
                )
                continue
            }

            // Changed row: payload is required.
            val typed: TypedPayload = projection.payload ?: continue
            val schemaId = typed.schemaId ?: continue
            // `payloadAsByteBuffer` is non-null (FlatBuffers returns an empty
            // buffer for an absent vector, never null).
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
                    projectionRev = projectionRev,
                    state = wireState,
                )
            )
        }
        return result
    }

}
