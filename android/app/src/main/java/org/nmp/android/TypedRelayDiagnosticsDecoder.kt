package org.nmp.android

import android.util.Log
import nmp.kernel.RelayDiagnosticsInterest as FbInterest
import nmp.kernel.RelayDiagnosticsRow as FbRow
import nmp.kernel.RelayDiagnosticsSnapshot as FbSnapshot
import nmp.kernel.RelayDiagnosticsWireSub as FbWireSub
import org.nmp.android.model.RelayDiagnosticsInterest
import org.nmp.android.model.RelayDiagnosticsRow
import org.nmp.android.model.RelayDiagnosticsSnapshot
import org.nmp.android.model.RelayDiagnosticsWireSub
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedRelayDiagnosticsDecoder"

/**
 * Typed-first decoder for the kernel-owned `relay_diagnostics` snapshot
 * projection (`KRDG` / [FbSnapshot]) — the Android peer of iOS
 * `TypedRelayDiagnosticsDecoder` + `TypedProjectionGlue.relayDiagnostics`.
 *
 * Pure field-for-field map of the relay diagnostics tree (rows → wireSubs;
 * interests → relayUrls). Every `*Label`/`*Tone` string is Rust-precomputed
 * (ADR-0032 / V-14) so `RelayScreen` renders `connectionLabel`/`connectionTone`
 * verbatim and never branches on raw protocol tokens. `has_*`-companion optional
 * strings lift to `null` when absent (byte-faithful to the JSON path).
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent fallback. Returns `null`
 * when the `KRDG` sidecar is absent / wrong schema / unverifiable, so the caller
 * keeps `relayDiagnostics = null`. Fail closed (D1) on a malformed buffer.
 */
object TypedRelayDiagnosticsDecoder {

    const val KEY = "relay_diagnostics"
    const val SCHEMA_ID = "relay_diagnostics"
    const val FILE_IDENTIFIER = "KRDG"

    fun decode(projections: List<TypedProjectionEnvelope>): RelayDiagnosticsSnapshot? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KRDG` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): RelayDiagnosticsSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbSnapshot.RelayDiagnosticsSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KRDG file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = FbSnapshot.getRootAsRelayDiagnosticsSnapshot(bb)
            val relays = ArrayList<RelayDiagnosticsRow>(snapshot.relaysLength)
            for (i in 0 until snapshot.relaysLength) {
                val row = snapshot.relays(i) ?: continue
                relays.add(mapRow(row))
            }
            val interests = ArrayList<RelayDiagnosticsInterest>(snapshot.interestsLength)
            for (i in 0 until snapshot.interestsLength) {
                val interest = snapshot.interests(i) ?: continue
                interests.add(mapInterest(interest))
            }
            RelayDiagnosticsSnapshot(relays = relays, interests = interests)
        } catch (e: Exception) {
            Log.e(TAG, "KRDG decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private fun mapRow(row: FbRow): RelayDiagnosticsRow {
        val wireSubs = ArrayList<RelayDiagnosticsWireSub>(row.wireSubsLength)
        for (j in 0 until row.wireSubsLength) {
            val sub = row.wireSubs(j) ?: continue
            wireSubs.add(mapWireSub(sub))
        }
        return RelayDiagnosticsRow(
            relayUrl = row.relayUrl ?: "",
            shortUrl = row.shortUrl ?: "",
            roleLabel = row.roleLabel ?: "",
            roleTone = row.roleTone ?: "",
            connectionLabel = row.connectionLabel ?: "",
            connectionTone = row.connectionTone ?: "",
            authLabel = row.authLabel ?: "",
            authTone = row.authTone ?: "",
            totalSubCount = row.totalSubCount.toInt(),
            activeSubCount = row.activeSubCount.toInt(),
            eosedSubCount = row.eosedSubCount.toInt(),
            totalEventsRx = row.totalEventsRx.toLong(),
            totalEventsDisplay = row.totalEventsDisplay ?: "",
            reconnectCount = row.reconnectCount.toInt(),
            bytesRxDisplay = if (row.hasBytesRxDisplay) row.bytesRxDisplay else null,
            bytesTxDisplay = if (row.hasBytesTxDisplay) row.bytesTxDisplay else null,
            lastConnectedDisplay = if (row.hasLastConnectedDisplay) row.lastConnectedDisplay else null,
            lastEventDisplay = if (row.hasLastEventDisplay) row.lastEventDisplay else null,
            lastNotice = if (row.hasLastNotice) row.lastNotice else null,
            lastError = if (row.hasLastError) row.lastError else null,
            wireSubs = wireSubs,
        )
    }

    private fun mapWireSub(sub: FbWireSub): RelayDiagnosticsWireSub = RelayDiagnosticsWireSub(
        wireId = sub.wireId ?: "",
        shortWireId = sub.shortWireId ?: "",
        relayUrl = sub.relayUrl ?: "",
        filterSummary = sub.filterSummary ?: "",
        stateLabel = sub.stateLabel ?: "",
        stateTone = sub.stateTone ?: "",
        consumerCountLabel = sub.consumerCountLabel ?: "",
        eventsRxDisplay = if (sub.hasEventsRxDisplay) sub.eventsRxDisplay else null,
        eoseObserved = sub.eoseObserved,
        openedDisplay = sub.openedDisplay ?: "",
        lastEventDisplay = if (sub.hasLastEventDisplay) sub.lastEventDisplay else null,
        eoseDisplay = if (sub.hasEoseDisplay) sub.eoseDisplay else null,
        closeReason = if (sub.hasCloseReason) sub.closeReason else null,
    )

    private fun mapInterest(interest: FbInterest): RelayDiagnosticsInterest {
        val urls = ArrayList<String>(interest.relayUrlsLength)
        for (k in 0 until interest.relayUrlsLength) {
            urls.add(interest.relayUrls(k) ?: continue)
        }
        return RelayDiagnosticsInterest(
            key = interest.key ?: "",
            state = interest.state ?: "",
            stateTone = interest.stateTone ?: "",
            refcount = interest.refcount.toInt(),
            cacheCoverage = interest.cacheCoverage ?: "",
            relayUrls = urls,
        )
    }
}
