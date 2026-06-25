package org.nmp.android

import android.util.Log
import nmp.nip47.WalletStatus as FbWalletStatus
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.text.NumberFormat
import java.util.Locale

private const val TAG = "TypedWalletDecoder"

/**
 * The wallet strings the Android UI renders off the `wallet` projection.
 *
 * RAW-DATA DOCTRINE (aim.md §2 / ADR-0032 /
 * docs/wiki/guides/shell-formatting-boundary.md): the kernel ships only the raw
 * `status` token and a raw `balance_sats:u64`. This shell derives the
 * `statusLabel`, `statusTone`, and thousands-separated `balanceDisplay` locally
 * — no presentation strings live on the wire. The earlier `status_label` /
 * `status_tone` / `balance_sats_display` precompute fields were a regression
 * (#623) removed in the wallet_status sweep (analogous to the #1580 signer-state
 * sweep).
 *
 * `WalletScreen` binds `statusLabel` verbatim and maps `statusTone` → colour.
 */
data class TypedWalletStrings(
    val status: String?,
    /** Thousands-separated balance, formatted shell-side from the raw
     *  `balance_sats`. `null` until the wallet responds to `get_balance`. */
    val balanceDisplay: String?,
    /** Display label derived from the raw `status` token. */
    val statusLabel: String,
    /** Semantic tone — "active"|"warning"|"error"|"inactive" — derived from
     *  the raw `status` token. */
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
 * Maps the wire `status` (required, always present) and the raw `balance_sats`
 * (formatted shell-side, `null`-when-absent) per the raw-data doctrine.
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
            TypedWalletStrings(
                status = rawStatus,
                // Format the raw `balance_sats` shell-side (raw-data doctrine).
                balanceDisplay = if (ws.hasBalanceSats) formatSats(ws.balanceSats) else null,
                statusLabel = deriveStatusLabel(rawStatus),
                statusTone = deriveStatusTone(rawStatus),
                // Rust-computed: bound verbatim, never re-derived in Kotlin.
                isConnected = ws.isConnected,
            )
        } catch (e: Exception) {
            Log.e(TAG, "NWST decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /**
     * Derive the display label from the raw wire token. Mirrors the parity
     * mapping shared with iOS `WalletStatusTone.label`.
     */
    private fun deriveStatusLabel(wire: String?): String = when (wire) {
        "connecting"   -> "Connecting"
        "ready"        -> "Ready"
        "error"        -> "Error"
        "disconnected" -> "Disconnected"
        else           -> "Unknown"
    }

    /**
     * Derive the semantic tone from the raw wire token. Mirrors the parity
     * mapping shared with iOS `WalletStatusTone.tone`.
     */
    private fun deriveStatusTone(wire: String?): String = when (wire) {
        "ready"      -> "active"
        "connecting" -> "warning"
        "error"      -> "error"
        else         -> "inactive"
    }

    /**
     * Format a satoshi count with thousands separators (`12345` → `"12,345"`),
     * mirroring iOS `WalletStatusTone.formattedSats`. Replaces the former
     * Rust-side `format_sats_display` precompute (raw-data doctrine).
     */
    private fun formatSats(sats: ULong): String =
        NumberFormat.getIntegerInstance(Locale.US).format(sats.toLong())
}
