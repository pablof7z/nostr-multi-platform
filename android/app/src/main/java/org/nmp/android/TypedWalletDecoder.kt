package org.nmp.android

import android.util.Log
import nmp.nip47.WalletStatus as FbWalletStatus
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedWalletDecoder"

/**
 * The wallet strings the Android UI renders off the `wallet` projection.
 *
 * ADR-0032 / #623: `statusLabel` and `statusTone` are pre-computed by Rust
 * so the UI never branches on raw protocol strings (thin-shell rule).
 *
 * `WalletScreen` binds `statusLabel` verbatim and maps `statusTone` → colour
 * without any `when`/`if` on the raw `status` token.
 */
data class TypedWalletStrings(
    val status: String?,
    val balanceDisplay: String?,
    /** ADR-0032 / #623: pre-computed label, e.g. "Connecting", "Ready". */
    val statusLabel: String,
    /** ADR-0032 / #623: semantic tone — "active"|"warning"|"error"|"inactive". */
    val statusTone: String,
    /** Rust-computed connected flag (`WalletStatus.is_connected`: status is
     *  "connecting" or "ready"). The shell binds this verbatim — it must NOT
     *  re-derive connectedness from `statusTone` (a native branch on a Rust
     *  wire discriminant; D7 / thin-shell). Mirrors iOS, which gates on
     *  `status.isConnected` (`WalletView.swift`). */
    val isConnected: Boolean,
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
 * Maps the wire `status` (required, always present) and
 * `has_balance_sats_display ? balance_sats_display : null` byte-faithfully
 * (ADR-0032 `null`-when-absent).
 *
 * Returns `null` when the `NWST` sidecar is absent / wrong schema /
 * unverifiable, so the typed-only host uses the empty wallet defaults. Fail
 * closed (D1/D6) on a malformed buffer.
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
            val ws = FbWalletStatus.getRootAsWalletStatus(bb)
            val rawStatus = ws.status
            // `statusLabel` / `statusTone` are tail-appended (additive). Absent
            // on older buffers that predate #623 — fall back to deriving from
            // `rawStatus` exactly as the Rust decode path does (D1).
            val label = ws.statusLabel ?: deriveStatusLabel(rawStatus)
            val tone  = ws.statusTone  ?: deriveStatusTone(rawStatus)
            TypedWalletStrings(
                status = rawStatus,
                balanceDisplay = if (ws.hasBalanceSatsDisplay) ws.balanceSatsDisplay else null,
                statusLabel = label,
                statusTone = tone,
                // Rust-computed: bound verbatim, never re-derived in Kotlin.
                isConnected = ws.isConnected,
            )
        } catch (e: Exception) {
            Log.e(TAG, "NWST decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /**
     * Fallback: derive the display label from the raw wire token.
     * Mirrors Rust `status_label()` — used only for pre-#623 buffers.
     */
    private fun deriveStatusLabel(wire: String?): String = when (wire) {
        "connecting"   -> "Connecting"
        "ready"        -> "Ready"
        "error"        -> "Error"
        "disconnected" -> "Disconnected"
        else           -> "Unknown"
    }

    /**
     * Fallback: derive the semantic tone from the raw wire token.
     * Mirrors Rust `status_tone()` — used only for pre-#623 buffers.
     */
    private fun deriveStatusTone(wire: String?): String = when (wire) {
        "ready"      -> "active"
        "connecting" -> "warning"
        "error"      -> "error"
        else         -> "inactive"
    }
}
