package org.nmp.android

import android.util.Log
import nmp.kernel.RelayDiagnosticsInfo as FbInfo
import nmp.kernel.RelayDiagnosticsInterest as FbInterest
import nmp.kernel.RelayDiagnosticsRow as FbRow
import nmp.kernel.RelayDiagnosticsSnapshot as FbSnapshot
import nmp.kernel.RelayDiagnosticsWireSub as FbWireSub
import org.nmp.android.model.RelayDiagnosticsInfo
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
            // aim.md §62: raw Unix-ms on wire; shells format at render time.
            lastConnectedMs = row.lastConnectedMs.toLong(),
            lastEventMs = row.lastEventMs.toLong(),
            lastNotice = if (row.hasLastNotice) row.lastNotice else null,
            lastError = if (row.hasLastError) row.lastError else null,
            wireSubs = wireSubs,
            // ADR-0051 — the NIP-11 info document is an OPTIONAL child table:
            // a null table means "no document fetched yet" (the JSON `info:
            // null` case), so table presence is the discriminator (no `has_info`
            // flag). Byte-faithful to the JSON path's `info: null`.
            info = row.info?.let(::mapInfo),
        )
    }

    private fun mapInfo(info: FbInfo): RelayDiagnosticsInfo {
        val nips = ArrayList<Int>(info.supportedNipsLength)
        for (n in 0 until info.supportedNipsLength) {
            nips.add(info.supportedNips(n).toInt())
        }
        return RelayDiagnosticsInfo(
            name = if (info.hasName) info.name else null,
            description = if (info.hasDescription) info.description else null,
            icon = if (info.hasIcon) info.icon else null,
            pubkey = if (info.hasPubkey) info.pubkey else null,
            contact = if (info.hasContact) info.contact else null,
            software = if (info.hasSoftware) info.software else null,
            version = if (info.hasVersion) info.version else null,
            supportedNips = nips,
            paymentRequired = if (info.hasPaymentRequired) info.paymentRequired else null,
            authRequired = if (info.hasAuthRequired) info.authRequired else null,
            restrictedWrites = if (info.hasRestrictedWrites) info.restrictedWrites else null,
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
        // aim.md §62: raw Unix-ms on wire; shells format at render time.
        openedMs = sub.openedMs.toLong(),
        lastEventMs = sub.lastEventMs.toLong(),
        eoseMs = sub.eoseMs.toLong(),
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
