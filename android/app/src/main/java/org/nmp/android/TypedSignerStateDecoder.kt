package org.nmp.android

import android.util.Log
import nmp.kernel.SignerState as FbSignerState
import org.nmp.android.model.SignerState
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedSignerStateDecoder"

/**
 * Typed-first decoder for the kernel-owned `signer_state` snapshot projection
 * (`KSST` / [FbSignerState]) — the Android peer of iOS `TypedSignerStateDecoder`
 * (`TypedProjectionDecoders.generated.swift`) + `TypedProjectionGlue.signerState`.
 *
 * ADR-0048 D6 (generalises V-14 / #963): unified remote-signer health. Covers
 * BOTH NIP-46 bunker and NIP-55 (Amber) sessions — `signerKind` discriminates.
 * Drives the signer health badge on the sign-in screen. Every flag and the
 * `statusLabel`/`statusTone` strings are Rust-precomputed (ADR-0032 / #1099) so
 * the UI never string-compares the raw `state` token.
 *
 * ADR-0037 Commitment 4: typed-FIRST with permanent fallback. Returns `null`
 * when the `KSST` sidecar is absent / wrong schema / unverifiable, so the caller
 * keeps `signerState = null` ("no remote-signer session"). Fail closed (D1/D6)
 * on a malformed buffer.
 *
 * `statusLabel`/`statusTone` are tail-appended (additive). Absent on pre-#1099
 * buffers → derive from the raw `state` token exactly as the Rust path does (D1).
 */
object TypedSignerStateDecoder {

    const val KEY = "signer_state"
    const val SCHEMA_ID = "signer_state"
    const val FILE_IDENTIFIER = "KSST"

    fun decode(projections: List<TypedProjectionEnvelope>): SignerState? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `KSST` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): SignerState? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbSignerState.SignerStateBufferHasIdentifier(bb)) {
                Log.e(TAG, "KSST file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val s = FbSignerState.getRootAsSignerState(bb)
            val rawState = s.state ?: ""
            SignerState(
                signerKind = s.signerKind ?: "",
                state = rawState,
                reason = if (s.hasReason) s.reason else null,
                isReady = s.isReady,
                isAwaitingApproval = s.isAwaitingApproval,
                isReconnecting = s.isReconnecting,
                isUnavailable = s.isUnavailable,
                isFailed = s.isFailed,
                statusLabel = s.statusLabel ?: deriveStatusLabel(rawState),
                statusTone = s.statusTone ?: deriveStatusTone(rawState),
            )
        } catch (e: Exception) {
            Log.e(TAG, "KSST decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /** Fallback label for pre-#1099 buffers; mirrors Rust `status_label()`. */
    private fun deriveStatusLabel(state: String): String = when (state) {
        "ready"             -> "Connected"
        "awaiting_approval" -> "Waiting for approval…"
        "reconnecting"      -> "Reconnecting…"
        "unavailable"       -> "Signer unavailable"
        "failed"            -> "Connection failed"
        else                -> "Unknown"
    }

    /** Fallback tone for pre-#1099 buffers; mirrors Rust `status_tone()`. */
    private fun deriveStatusTone(state: String): String = when (state) {
        "ready"                            -> "active"
        "awaiting_approval", "reconnecting" -> "warning"
        "unavailable", "failed"            -> "error"
        else                               -> "inactive"
    }
}
