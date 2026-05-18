package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Kotlin port of the Swift `EmbedEntry` + nested DTOs in
 * `ios/NmpGallery/NmpGallery/GalleryEmbedDto.swift`. Mirrors the embed half
 * of `crates/nmp-content-fixtures/src/dto.rs`.
 */
@Serializable
data class EmbedEntry(
    @SerialName("resolved_kind") val resolvedKind: Int = 0,
    @SerialName("profile_name") val profileName: String? = null,
    @SerialName("profile_picture") val profilePicture: String? = null,
    val event: SignedEventJson? = null,
    val rendered: ContentTreeDto? = null,
    val collapsed: Boolean = false,
    @SerialName("collapse_reason") val collapseReason: String? = null,
    val article: ArticleHeaderDto? = null,
    val list: ListDto? = null,
)

@Serializable
data class ArticleHeaderDto(
    val title: String? = null,
    val summary: String? = null,
    val author: String,
    @SerialName("d_tag") val dTag: String,
)

@Serializable
data class ListDto(
    val title: String? = null,
    val rows: List<ListRowDto>,
)

@Serializable(with = ListRowDtoSerializer::class)
sealed class ListRowDto {
    @Serializable
    @SerialName("profile")
    data class Profile(
        val pubkey: String,
        val name: String? = null,
        val picture: String? = null,
    ) : ListRowDto()

    @Serializable
    @SerialName("event")
    data class Event(val id: String) : ListRowDto()

    @Serializable
    @SerialName("address")
    data class Address(val coord: String) : ListRowDto()

    @Serializable
    @SerialName("hashtag")
    data class Hashtag(val tag: String) : ListRowDto()

    @Serializable
    @SerialName("relay")
    data class Relay(
        val url: String,
        val read: Boolean,
        val write: Boolean,
    ) : ListRowDto()

    @Serializable
    @SerialName("unknown")
    data class Unknown(val type: String = "unknown") : ListRowDto()
}

internal object ListRowDtoSerializer :
    JsonContentPolymorphicSerializer<ListRowDto>(ListRowDto::class) {
    override fun selectDeserializer(element: JsonElement) =
        when (element.jsonObject["type"]?.jsonPrimitive?.content) {
            "profile" -> ListRowDto.Profile.serializer()
            "event" -> ListRowDto.Event.serializer()
            "address" -> ListRowDto.Address.serializer()
            "hashtag" -> ListRowDto.Hashtag.serializer()
            "relay" -> ListRowDto.Relay.serializer()
            else -> ListRowDto.Unknown.serializer()
        }
}
