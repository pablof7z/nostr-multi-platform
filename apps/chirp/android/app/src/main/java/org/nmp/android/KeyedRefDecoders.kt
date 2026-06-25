package org.nmp.android

import nmp.kernel.ClaimedEventsSnapshot
import nmp.kernel.ProfileCard as FbProfileCard
import nmp.kernel.ProfileSnapshot
import org.nmp.android.model.ProfileCard

/**
 * ADR-0063 Lane G (#1671) — HAND-WRITTEN reader→domain glue for the generated
 * `KeyedRefCache` typed accessors. The Android twin of the Swift
 * `TypedProjectionGlue.profile` / `TypedProjectionGlue+Refs.refRowEvent`.
 *
 * The generated `KeyedRefCache.decodeProfileRow` / `decodeEventRow` helpers do
 * the CHECKED root decode (verifying the KPRF / KCEV file_identifier) then hand
 * the reader here to map it to the Chirp domain type. Kept out of the generated
 * file (which is regenerated from `KEYED_PROJECTIONS`) so the wire→domain mapping
 * stays hand-authored exactly like iOS.
 */
object KeyedRefDecoders {

    /**
     * Map the typed `refs.profile` row payload (`KPRF` / [ProfileSnapshot]) to the
     * [ProfileCard] domain type — the SAME value the JSON `projections.profile`
     * path yields (the active account's card). The Swift twin is
     * `TypedProjectionGlue.profile`. Returns `null` when the snapshot carries no
     * `card` (fail closed at the decode-before-commit seam).
     */
    fun refRowProfile(reader: ProfileSnapshot): ProfileCard? {
        val card = reader.card ?: return null
        return mapProfileCard(card)
    }

    /**
     * Map ONE `refs.event` row payload buffer — a `KCEV`
     * [ClaimedEventsSnapshot] carrying EXACTLY ONE entry — to that one
     * [ClaimedEventDto]. The per-row twin of the whole-map claimed-events path;
     * the [KeyedRefCache.event] accessor calls this so a view binds one decoded
     * event, not a map.
     *
     * FAIL-CLOSED single-entry contract (mirrors the Swift `refRowEvent`): the
     * kernel's `ref_event_row_payload` encodes EXACTLY ONE entry per `refs.event`
     * row, always. A buffer with 0 OR 2+ entries is MALFORMED — returning the
     * first of several would let a corrupt multi-entry KCEV pass
     * decode-before-commit and silently commit the wrong row. We require
     * `entriesLength == 1` and return `null` otherwise, so the
     * decode-before-commit seam rejects the row (prior row retained, needsResync
     * latched) rather than committing a forged event.
     */
    fun refRowEvent(reader: ClaimedEventsSnapshot): ClaimedEventDto? {
        if (reader.entriesLength != 1) return null
        val entry = reader.entries(0) ?: return null
        val event = entry.value ?: return null
        val tags = ArrayList<List<String>>(event.tagsLength)
        for (i in 0 until event.tagsLength) {
            val tagRow = event.tags(i) ?: continue
            val values = ArrayList<String>(tagRow.valuesLength)
            for (j in 0 until tagRow.valuesLength) {
                values.add(tagRow.values(j) ?: "")
            }
            tags.add(values)
        }
        return ClaimedEventDto(
            id = event.id ?: "",
            authorPubkey = event.authorPubkey ?: "",
            kind = event.kind.toInt(),
            createdAt = event.createdAt.toLong(),
            content = event.content ?: "",
            tags = tags,
            signedEventJson = if (event.hasSignedEventJson) event.signedEventJson ?: "" else null,
        )
    }

    /**
     * Map a shared [FbProfileCard] wire row to the domain [ProfileCard]. The
     * `has_*` companion bools reproduce the JSON `null`-when-absent semantics
     * (ADR-0032): `has_display_name == false` -> `displayName = null`, etc.
     * ADR-0063 Lane H: `TypedProfilesDecoder` deleted; profile data now served
     * via the refs.profile KPRF NRRD row-delta sidecar. Swift peer:
     * `TypedProjectionGlue.profileCard`.
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

/**
 * ADR-0063 Lane G (#1671) — the Android domain type the `refs.event` typed
 * accessor returns: one resolved+enriched event, the Kotlin twin of the Swift
 * `ClaimedEventDto`. Field-for-field the render-relevant subset of one
 * `nmp.kernel.ClaimedEvent` row (the author display-name / picture-url enrich
 * fields are reachable via the per-key `refs.profile` cache, not duplicated here,
 * matching the Swift struct). `signedEventJson` is populated only for the
 * generic `event.raw` row shape.
 */
data class ClaimedEventDto(
    val id: String,
    val authorPubkey: String,
    val kind: Int,
    val createdAt: Long,
    val content: String,
    val tags: List<List<String>>,
    val signedEventJson: String? = null,
)
