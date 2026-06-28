// Requires: kotlinx.serialization. Kotlin 1.9+.
//
// Kotlin kotlinx.serialization mirror of the Rust
// `nmp_content::embed_projection::EmbeddedEventEnvelope` / `EmbedKindProjection`.
//
// The Rust enum is serialized with `#[serde(tag = "variant", content = "data",
// rename_all = "camelCase")]`, but the typed NEMB sidecar decoder on Android
// populates exactly one variant field (`shortNote` / `article` / …). This file
// models that already-decoded shape: the dispatch decision is a `when` over
// which variant is non-null. No protocol parsing happens here — the kernel
// resolved the event into a typed projection before it reached this layer.
//
// Compose mirror of the SwiftUI `EmbedKindProjection.swift`.

package org.nmp.gallery.registry

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Kind-dispatched embed projection — exactly one variant is non-null, selected
 * by the kernel's resolver from the `kind` discriminant. Mirrors the SwiftUI
 * `EmbedKindProjection` enum. DECODE-ONLY: no resolution logic lives here.
 */
@Serializable
public data class EmbedKindProjection(
    @SerialName("short_note") val shortNote: ShortNoteProjection? = null,
    val article: ArticleProjection? = null,
    val highlight: HighlightProjection? = null,
    val profile: ProfileProjection? = null,
    val unknown: UnknownProjection? = null,
)

/** kind:1 short text note projection. */
@Serializable
public data class ShortNoteProjection(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    /** Plain-text fallback for the content body. */
    val content: String = "",
    @SerialName("media_urls") val mediaUrls: List<String> = emptyList(),
)

/** kind:30023 long-form article projection (NIP-23). */
@Serializable
public data class ArticleProjection(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    val title: String? = null,
    val summary: String? = null,
    @SerialName("hero_image_url") val heroImageUrl: String? = null,
    @SerialName("d_tag") val dTag: String = "",
    val content: String = "",
)

/** kind:9802 highlight projection (NIP-84). */
@Serializable
public data class HighlightProjection(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    @SerialName("highlighted_text") val highlightedText: String = "",
    @SerialName("source_event_id") val sourceEventId: String? = null,
    @SerialName("source_event_addr") val sourceEventAddr: String? = null,
    @SerialName("source_url") val sourceUrl: String? = null,
    val context: String? = null,
)

/** kind:0 profile metadata projection. */
@Serializable
public data class ProfileProjection(
    val pubkey: String = "",
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("picture_url") val pictureUrl: String? = null,
    val about: String? = null,
    val nip05: String? = null,
    val lud16: String? = null,
    @SerialName("banner_url") val bannerUrl: String? = null,
)

/** Fallback projection for kinds without a registered handler. */
@Serializable
public data class UnknownProjection(
    val kind: Int = 0,
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    val content: String = "",
    val tags: List<List<String>> = emptyList(),
    @SerialName("alt_text") val altText: String? = null,
)

/**
 * Full envelope mirror of `nmp_content::embed_projection::EmbeddedEventEnvelope`.
 * Populated exclusively from the typed NEMB sidecar (not a JSON projection).
 */
@Serializable
public data class EmbeddedEventEnvelope(
    /** The original nostr: URI (nevent1… / naddr1… / npub1…). */
    val uri: String = "",
    /** Primary identifier: event-id hex, or `kind:pubkey:d` coordinate. */
    @SerialName("primary_id") val primaryId: String = "",
    /** Recursion guard state (reserved for nested-embed depth limits). */
    val depth: Int = 0,
    @SerialName("max_depth") val maxDepth: Int = 4,
    /** Kind-dispatched projection — drives which renderer is chosen. */
    val projection: EmbedKindProjection? = null,
    /** Whether this embed should be collapsed (depth limit, cycle, unsupported). */
    val collapsed: Boolean = false,
    /** Optional machine-readable collapse reason. */
    @SerialName("collapse_reason") val collapseReason: String? = null,
)
