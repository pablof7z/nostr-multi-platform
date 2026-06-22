package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator

/**
 * Gallery-bundle embed envelope: collapsed flag + kind-registry projection.
 *
 * Mirrors the production `EmbedEnvelopeEntry` / `EmbeddedEventEnvelope` shape
 * but decoded from JSON (the gallery reads a pre-computed asset bundle rather
 * than a live NEMB FlatBuffers sidecar). The bundle version 3 format ships one
 * of these per `nostr:` URI in the `embeds` map.
 *
 * THIN-SHELL (F-CR-04): no kind dispatch or protocol logic here; the Rust
 * `resolve_embed_projection` call in the bundle builder already selected the
 * correct `GalleryEmbedKindProjection` variant.
 */
@Serializable
data class GalleryEmbedEnvelope(
    val collapsed: Boolean = false,
    @SerialName("collapse_reason") val collapseReason: String? = null,
    val projection: GalleryEmbedKindProjection? = null,
)

/**
 * Kind-dispatched embed projection — exactly one variant is selected by the
 * Rust `resolve_embed_projection` resolver. The `variant` field (the serde
 * discriminant) drives gallery kind-registry dispatch; the `data` block carries
 * the typed payload.
 *
 * Mirrors the Rust `EmbedKindProjection` enum (variants.rs) at the JSON wire
 * level: `{"variant": "shortNote", "data": {...}}`.
 */
@OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
@Serializable
@JsonClassDiscriminator("variant")
sealed class GalleryEmbedKindProjection {

    @Serializable
    @SerialName("shortNote")
    data class ShortNote(val data: ShortNoteProjection) : GalleryEmbedKindProjection()

    @Serializable
    @SerialName("article")
    data class Article(val data: ArticleProjection) : GalleryEmbedKindProjection()

    @Serializable
    @SerialName("highlight")
    data class Highlight(val data: HighlightProjection) : GalleryEmbedKindProjection()

    @Serializable
    @SerialName("profile")
    data class Profile(val data: ProfileProjection) : GalleryEmbedKindProjection()

    @Serializable
    @SerialName("unknown")
    data class Unknown(val data: UnknownProjection) : GalleryEmbedKindProjection()
}

/** kind:1 short text note projection (mirrors Rust `ShortNoteProjection`). */
@Serializable
data class ShortNoteProjection(
    val id: String = "",
    val authorPubkey: String = "",
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val createdAt: Long = 0,
    /** Rendered content tree for the note body. */
    val contentTree: ContentTreeWire = ContentTreeWire(),
    val mediaUrls: List<String> = emptyList(),
)

/** kind:30023 long-form article projection (mirrors Rust `ArticleProjection`). */
@Serializable
data class ArticleProjection(
    val id: String = "",
    val authorPubkey: String = "",
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val createdAt: Long = 0,
    val title: String? = null,
    val summary: String? = null,
    val heroImageUrl: String? = null,
    val dTag: String = "",
    val contentTree: ContentTreeWire = ContentTreeWire(),
)

/** kind:9802 highlight projection (mirrors Rust `HighlightProjection`). */
@Serializable
data class HighlightProjection(
    val id: String = "",
    val authorPubkey: String = "",
    val authorDisplayName: String? = null,
    val createdAt: Long = 0,
    val highlightedText: String = "",
    val sourceEventId: String? = null,
    val sourceEventAddr: String? = null,
    val sourceUrl: String? = null,
    val context: String? = null,
)

/** kind:0 profile metadata projection (mirrors Rust `ProfileProjection`). */
@Serializable
data class ProfileProjection(
    val pubkey: String = "",
    val displayName: String? = null,
    val pictureUrl: String? = null,
    val about: String? = null,
    val nip05: String? = null,
    val lud16: String? = null,
    val bannerUrl: String? = null,
)

/** Fallback projection for unknown kinds (mirrors Rust `UnknownProjection`). */
@Serializable
data class UnknownProjection(
    val kind: Int = 0,
    val authorPubkey: String = "",
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val createdAt: Long = 0,
    val content: String = "",
    val contentTree: ContentTreeWire = ContentTreeWire(),
    val tags: List<List<String>> = emptyList(),
    val altText: String? = null,
)
