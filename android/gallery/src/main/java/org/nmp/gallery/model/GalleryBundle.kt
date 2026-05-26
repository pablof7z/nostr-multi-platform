package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Root Android gallery bundle. Content is already projected by Rust into the
 * canonical ContentTreeWire arena; Kotlin decodes and renders only.
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
    val rendered: ContentTreeWire,
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
