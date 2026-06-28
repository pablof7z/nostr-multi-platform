package org.nmp.android

import android.util.Log
import nmp.kernel.AccountSummaryRow
import nmp.kernel.AccountsSnapshot
import nmp.kernel.ActiveAccountSnapshot
import org.nmp.android.model.AccountSummary
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedAccountsDecoder"

/**
 * Presence wrapper for the typed `active_account` (`KACT`) decode. The domain
 * value is a nullable `String?` where `null` means "no account active" — which
 * collides with the "no typed sidecar" sentinel. So the decoder returns a
 * non-null [ActiveAccountResult] when (and only when) a usable `KACT` sidecar is
 * present; a null wrapper means "no usable typed sidecar". The inner
 * [pubkey] then faithfully carries `null` (JSON `null`, `has_active_account ==
 * false`) without being confused for absence. Mirrors the wallet decoder's
 * `TypedWalletStrings?` presence pattern, and iOS
 * `TypedActiveAccountDecoder` + `TypedProjectionGlue.activeAccount`.
 */
data class ActiveAccountResult(val pubkey: String?)

/**
 * Typed-first decoder for the two kernel-owned account-cluster snapshot
 * projections — the Android peer of iOS `TypedAccountsDecoder` /
 * `TypedActiveAccountDecoder` (`TypedProjectionDecoders.generated.swift`) plus
 * the `TypedProjectionGlue.accounts` / `activeAccount` wire→domain glue:
 *
 *  - `accounts` (`KACC` / [AccountsSnapshot]) → `List<AccountSummary>`
 *  - `active_account` (`KACT` / [ActiveAccountSnapshot]) → `String?`
 *
 * The producer is `Kernel::snapshot_projections_with_publish_cluster`
 * (`crates/nmp-core/src/kernel/update/projections.rs`), which emits these typed
 * buffers under the account-cluster keys.
 *
 * [decodeAccounts] returns `null`, and [decodeActiveAccount] returns `null`,
 * when the matching sidecar is absent, carries the wrong schema id, or is an
 * unverifiable buffer; the caller then uses the typed-only empty/null default for
 * that projection. A malformed sidecar yields `null` (fail closed, D1/D6 — never
 * a partial or stale value).
 *
 * Field mapping note (#979): the `AccountSummaryRow` carries the FULL `npub`
 * (bech32). The Android [AccountSummary] domain type now also carries the full
 * `npub` (the old `npubShort` read a JSON key the kernel never emits, so it was
 * always empty — see aim.md §2). Abbreviation is a Compose-layer concern,
 * exactly as iOS does (`account.npub.shortHex`, PR #1064). The `has_*`
 * companion bools (`has_display_name`) reproduce the JSON `null`-when-absent
 * semantics, but the Android domain subset stores `displayName` as a plain
 * (defaulted-empty) String — so `has_display_name == false` maps to `""`,
 * byte-faithful to the generic `decodeAccountSummary` path.
 */
object TypedAccountsDecoder {

    const val ACCOUNTS_KEY = "accounts"
    const val ACCOUNTS_SCHEMA_ID = "accounts"
    const val ACCOUNTS_FILE_IDENTIFIER = "KACC"

    const val ACTIVE_ACCOUNT_KEY = "active_account"
    const val ACTIVE_ACCOUNT_SCHEMA_ID = "active_account"
    const val ACTIVE_ACCOUNT_FILE_IDENTIFIER = "KACT"

    /**
     * Decode the typed `accounts` sidecar into the Android domain list. `null`
     * when no usable `KACC` sidecar is present.
     */
    fun decodeAccounts(projections: List<TypedProjectionEnvelope>): List<AccountSummary>? {
        val payload = selectPayload(projections, ACCOUNTS_KEY, ACCOUNTS_SCHEMA_ID) ?: return null
        return decodeAccountsBytes(payload)
    }

    /**
     * Decode the typed `active_account` sidecar. Returns a non-null
     * [ActiveAccountResult] only when a usable `KACT` sidecar is present (its
     * [ActiveAccountResult.pubkey] then carries the authoritative `String?`);
     * `null` means "no usable typed sidecar".
     */
    fun decodeActiveAccount(projections: List<TypedProjectionEnvelope>): ActiveAccountResult? {
        val payload = selectPayload(projections, ACTIVE_ACCOUNT_KEY, ACTIVE_ACCOUNT_SCHEMA_ID)
            ?: return null
        return decodeActiveAccountBytes(payload)
    }

    /** Locate the matching envelope's non-empty payload bytes, or `null`. */
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

    /** Decode a raw `KACC` buffer into `List<AccountSummary>`; `null` on any failure. */
    fun decodeAccountsBytes(bytes: ByteArray): List<AccountSummary>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!AccountsSnapshot.AccountsSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KACC file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = AccountsSnapshot.getRootAsAccountsSnapshot(bb)
            val result = ArrayList<AccountSummary>(snapshot.accountsLength)
            for (i in 0 until snapshot.accountsLength) {
                val row = snapshot.accounts(i) ?: continue
                result.add(mapAccountSummary(row))
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "KACC decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /** Decode a raw `KACT` buffer into an [ActiveAccountResult]; `null` on any failure. */
    fun decodeActiveAccountBytes(bytes: ByteArray): ActiveAccountResult? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!ActiveAccountSnapshot.ActiveAccountSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KACT file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = ActiveAccountSnapshot.getRootAsActiveAccountSnapshot(bb)
            // `has_active_account == false` mirrors JSON `null`.
            ActiveAccountResult(if (snapshot.hasActiveAccount) snapshot.pubkey ?: "" else null)
        } catch (e: Exception) {
            Log.e(TAG, "KACT decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /**
     * Map a typed [AccountSummaryRow] to the Android domain [AccountSummary].
     * Carries the full `npub`; `has_display_name == false` -> `displayName =
     * ""` (the Android subset stores a defaulted-empty String).
     */
    private fun mapAccountSummary(row: AccountSummaryRow): AccountSummary = AccountSummary(
        id = row.id ?: "",
        npub = row.npub ?: "",
        displayName = if (row.hasDisplayName) row.displayName ?: "" else "",
        status = row.status ?: "",
        // `signer_label` was removed from the wire (#1712); the model derives it
        // shell-side from the raw `signerKind` token.
        signerKind = row.signerKind ?: "",
    )
}
