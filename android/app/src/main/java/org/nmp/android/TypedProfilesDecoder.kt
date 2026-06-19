package org.nmp.android

import android.util.Log
import nmp.kernel.ClaimedProfilesSnapshot
import nmp.kernel.ProfileCard as FbProfileCard
import nmp.kernel.ResolvedProfilesSnapshot
import org.nmp.android.model.ProfileCard
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedProfilesDecoder"

/**
 * Typed-first decoder for the kernel-owned profile-cluster snapshot projections
 * `resolved_profiles` (`KRPR` / `ResolvedProfilesSnapshot`) and
 * `claimed_profiles` (`KCPR` / `ClaimedProfilesSnapshot`) — the Android peer of
 * iOS `TypedResolvedProfilesDecoder` / `TypedClaimedProfilesDecoder`
 * (`TypedProjectionDecoders.generated.swift`) plus the
 * `TypedProjectionGlue.resolvedProfiles` / `claimedProfiles` wire→domain glue.
 *
 * Both wires flatten the producer `BTreeMap<String, ProfileCard>` to a
 * `[{key, value}]` vector (FlatBuffers has no map type); each entry is rebuilt
 * into the Android `Map<String, ProfileCard>` domain shape.
 * The shared [FbProfileCard] row type is `include`d from `profile_card.fbs`,
 * so a single [mapProfileCard] handles both clusters.
 *
 * [decodeResolved] / [decodeClaimed] return `null` when the matching sidecar is
 * absent, carries the wrong schema id / version, or is an un-verifiable buffer;
 * the caller ([KernelUpdateFrameDecoder.decodeProjections]) then uses the
 * typed-only empty map default. A malformed sidecar yields `null` (fail closed,
 * D1/D6 — never a partial or stale map).
 */
object TypedProfilesDecoder {

    const val RESOLVED_KEY = "resolved_profiles"
    const val RESOLVED_SCHEMA_ID = "resolved_profiles"
    const val RESOLVED_FILE_IDENTIFIER = "KRPR"

    const val CLAIMED_KEY = "claimed_profiles"
    const val CLAIMED_SCHEMA_ID = "claimed_profiles"
    const val CLAIMED_FILE_IDENTIFIER = "KCPR"

    private const val SUPPORTED_SCHEMA_VERSION: UInt = 2u

    /**
     * Decode the typed `resolved_profiles` sidecar into the pubkey -> card map.
     * `null` when no usable `KRPR` sidecar is present (caller falls back).
     */
    fun decodeResolved(projections: List<TypedProjectionEnvelope>): Map<String, ProfileCard>? {
        val payload = selectPayload(projections, RESOLVED_KEY, RESOLVED_SCHEMA_ID) ?: return null
        return decodeResolvedBytes(payload)
    }

    /**
     * Decode the typed `claimed_profiles` sidecar into the pubkey -> card map.
     * `null` when no usable `KCPR` sidecar is present (caller falls back).
     */
    fun decodeClaimed(projections: List<TypedProjectionEnvelope>): Map<String, ProfileCard>? {
        val payload = selectPayload(projections, CLAIMED_KEY, CLAIMED_SCHEMA_ID) ?: return null
        return decodeClaimedBytes(payload)
    }

    /** Locate the matching envelope's payload bytes, or `null` when unusable. */
    private fun selectPayload(
        projections: List<TypedProjectionEnvelope>,
        key: String,
        schemaId: String,
    ): ByteArray? {
        val projection = projections.firstOrNull {
            it.key == key && it.schemaId == schemaId
        } ?: return null
        if (projection.schemaVersion != SUPPORTED_SCHEMA_VERSION) return null
        if (projection.payload.isEmpty()) return null
        return projection.payload
    }

    /** Decode a raw `KRPR` buffer; `null` on any parse failure. */
    fun decodeResolvedBytes(bytes: ByteArray): Map<String, ProfileCard>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!ResolvedProfilesSnapshot.ResolvedProfilesSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KRPR file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = ResolvedProfilesSnapshot.getRootAsResolvedProfilesSnapshot(bb)
            val result = LinkedHashMap<String, ProfileCard>(snapshot.entriesLength * 2)
            for (i in 0 until snapshot.entriesLength) {
                val entry = snapshot.entries(i) ?: continue
                val key = entry.key ?: continue
                val card = entry.value ?: continue
                result[key] = mapProfileCard(card)
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "KRPR decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /** Decode a raw `KCPR` buffer; `null` on any parse failure. */
    fun decodeClaimedBytes(bytes: ByteArray): Map<String, ProfileCard>? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!ClaimedProfilesSnapshot.ClaimedProfilesSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "KCPR file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = ClaimedProfilesSnapshot.getRootAsClaimedProfilesSnapshot(bb)
            val result = LinkedHashMap<String, ProfileCard>(snapshot.entriesLength * 2)
            for (i in 0 until snapshot.entriesLength) {
                val entry = snapshot.entries(i) ?: continue
                val key = entry.key ?: continue
                val card = entry.value ?: continue
                result[key] = mapProfileCard(card)
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "KCPR decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    /**
     * Map a shared [FbProfileCard] wire row to the domain [ProfileCard]. The
     * `has_*` companion bools reproduce the JSON `null`-when-absent semantics
     * (ADR-0032): `has_display_name == false` -> `displayName = null`, etc.,
     * byte-faithful to the generic `decodeProfileCard` path.
     */
    private fun mapProfileCard(card: FbProfileCard): ProfileCard = ProfileCard(
        pubkey = card.pubkey ?: "",
        // V-115 / ADR-0032: `npub` removed from profile_card.fbs; bech32
        // encoding is host-side via KernelBridge.encodeProfile.
        npub = "",
        displayName = if (card.hasDisplayName) card.displayName else null,
        name = if (card.hasName) card.name else null,
        rawDisplayName = if (card.hasRawDisplayName) card.rawDisplayName else null,
        displayNameCamel = if (card.hasDisplayNameCamel) card.displayNameCamel else null,
        pictureUrl = if (card.hasPictureUrl) card.pictureUrl else null,
        banner = if (card.hasBanner) card.banner else null,
        website = if (card.hasWebsite) card.website else null,
        nip05 = card.nip05 ?: "",
        about = card.about ?: "",
        lud16 = if (card.hasLud16) card.lud16 else null,
        lud06 = if (card.hasLud06) card.lud06 else null,
        lnurl = if (card.hasLnurl) card.lnurl else null,
    )
}
