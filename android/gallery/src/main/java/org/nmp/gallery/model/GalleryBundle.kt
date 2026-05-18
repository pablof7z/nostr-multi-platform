package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Kotlin mirror of `crates/nmp-content-fixtures/src/dto.rs`, port of the
 * Swift `GalleryBundle` in `ios/NmpGallery/NmpGallery/GalleryBundle.swift`.
 *
 * PROJECTION-GAP NOTE: `nmp_content::Segment` / `ContentTree` /
 * `MarkdownNode` are deliberately non-serde with no FFI projection (T93).
 * STAGE 2 projects them to a serde JSON mirror; this file is the Kotlin
 * decode side of that mirror. The `type` discriminator matches serde's
 * `#[serde(tag = "type", rename_all = "camelCase")]` — variant names are
 * camelCase, field names stay snake_case.
 */
@Serializable
data class GalleryBundle(
    val version: Int,
    val scenarios: List<Scenario>,
)

@Serializable
data class Scenario(
    val id: String,
    val category: String,
    val title: String,
    val exercises: String,
    val events: List<SignedEventJson>,
    val rendered: ContentTreeDto,
    val embeds: Map<String, EmbedEntry>,
)

@Serializable
data class SignedEventJson(
    val id: String,
    val pubkey: String,
    @SerialName("created_at") val createdAt: Long,
    val kind: Int,
    val tags: List<List<String>>,
    val content: String,
    val sig: String,
)

@Serializable
data class ContentTreeDto(
    val mode: String,
    val segments: List<SegmentDto>,
)
