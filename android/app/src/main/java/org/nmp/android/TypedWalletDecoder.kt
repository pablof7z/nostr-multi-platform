package org.nmp.android

import android.util.Log
import nmp.nip47.WalletStatus as FbWalletStatus
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedWalletDecoder"

/**
 * The two wallet strings the Android UI renders off the `wallet` projection:
 * `WalletScreen` reads `walletStatus` (the connection status token) and
 * `walletBalance` (the pre-formatted sats display string). The full
 * `WalletStatus` wire carries more (npub, msats, connection state), but the
 * generic `payload:Value` path only ever surfaced these two — so the typed
 * decoder surfaces exactly them to keep typed/fallback observably identical.
 */
data class TypedWalletStrings(
    val status: String?,
    val balanceDisplay: String?,
)

/**
 * Typed-first decoder for the NIP-47 `wallet` snapshot projection (`NWST` /
 * `WalletStatus`) — the Android peer of iOS `TypedWalletDecoder`
 * (`TypedProjectionDecoders.generated.swift`) + `TypedProjectionGlue.wallet`.
 *
 * Note the key/schema-id asymmetry: the projection KEY is `wallet`, but the
 * `TypedPayload.schema_id` is `nmp.nip47.wallet` (the producer registers it that
 * way — see `wallet_runtime.rs`). The decoder matches on BOTH.
 *
 * The generic path produced `walletStatus = m["status"]` and
 * `walletBalance = m["balanceSatsDisplay"]`; this maps the wire `status`
 * (required, always present) and `has_balance_sats_display ? balance_sats_display
 * : null` byte-faithfully (ADR-0032 `null`-when-absent).
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent generic fallback. Returns
 * `null` when the `NWST` sidecar is absent / wrong schema / unverifiable, so the
 * caller keeps the generic `payload:Value` `wallet` subtree. Fail closed
 * (D1/D6) on a malformed buffer.
 */
object TypedWalletDecoder {

    const val KEY = "wallet"
    const val SCHEMA_ID = "nmp.nip47.wallet"
    const val FILE_IDENTIFIER = "NWST"

    fun decode(projections: List<TypedProjectionEnvelope>): TypedWalletStrings? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `NWST` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): TypedWalletStrings? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbWalletStatus.WalletStatusBufferHasIdentifier(bb)) {
                Log.e(TAG, "NWST file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val status = FbWalletStatus.getRootAsWalletStatus(bb)
            TypedWalletStrings(
                status = status.status,
                balanceDisplay = if (status.hasBalanceSatsDisplay) status.balanceSatsDisplay else null,
            )
        } catch (e: Exception) {
            Log.e(TAG, "NWST decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }
}
