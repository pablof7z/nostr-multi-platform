package org.nmp.gallery.bridge

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Real Nostr references shared by every NmpGallery host.
 *
 * Rust embeds `apps/nmp-gallery/showcase-references.json` and Android reads
 * that same JSON through JNI. Kotlin does not duplicate pubkeys, event ids,
 * URIs, or relay roles.
 */
@Serializable
data class GalleryShowcaseReferences(
    @SerialName("schema") val schema: String,
    @SerialName("profile") val profile: GalleryShowcaseProfile,
    @SerialName("article") val article: GalleryShowcaseEvent,
    @SerialName("note") val note: GalleryShowcaseEvent,
    @SerialName("highlight") val highlight: GalleryShowcaseEvent,
    @SerialName("relays") val relays: List<GalleryShowcaseRelay>,
) {
    companion object {
        fun decode(json: String): GalleryShowcaseReferences =
            Json { ignoreUnknownKeys = true }.decodeFromString(json)
    }
}

@Serializable
data class GalleryShowcaseProfile(
    @SerialName("pubkey_hex") val pubkeyHex: String,
    @SerialName("npub") val npub: String,
    @SerialName("npub_short") val npubShort: String,
)

@Serializable
data class GalleryShowcaseEvent(
    @SerialName("uri") val uri: String,
    @SerialName("primary_id") val primaryId: String,
    @SerialName("kind") val kind: Long,
    @SerialName("label") val label: String,
    @SerialName("expected_title") val expectedTitle: String? = null,
)

@Serializable
data class GalleryShowcaseRelay(
    @SerialName("url") val url: String,
    @SerialName("role") val role: String,
)
