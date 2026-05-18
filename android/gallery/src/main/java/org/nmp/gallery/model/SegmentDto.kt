package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Kotlin port of the Swift `SegmentDto` enum. Uses
 * `JsonContentPolymorphicSerializer` so unknown discriminator values fall
 * back to [Unknown] instead of throwing — matches the Swift `.unknown(type)`
 * case and the Rust `Segment::Unknown` projection contract.
 */
@Serializable(with = SegmentDtoSerializer::class)
sealed class SegmentDto {
    @Serializable
    @SerialName("text")
    data class Text(val text: String) : SegmentDto()

    @Serializable
    @SerialName("mention")
    data class Mention(
        val uri: String,
        val kind: String,
        val pubkey: String,
    ) : SegmentDto()

    @Serializable
    @SerialName("eventRef")
    data class EventRef(
        val uri: String,
        val kind: String,
        val id: String,
    ) : SegmentDto()

    @Serializable
    @SerialName("hashtag")
    data class Hashtag(val tag: String) : SegmentDto()

    @Serializable
    @SerialName("url")
    data class Url(val url: String) : SegmentDto()

    @Serializable
    @SerialName("media")
    data class Media(
        @SerialName("media_kind") val mediaKind: String,
        val urls: List<String>,
    ) : SegmentDto()

    @Serializable
    @SerialName("emoji")
    data class Emoji(
        val shortcode: String,
        val url: String? = null,
    ) : SegmentDto()

    @Serializable
    @SerialName("invoice")
    data class Invoice(
        @SerialName("invoice_kind") val invoiceKind: String,
        val value: String,
    ) : SegmentDto()

    @Serializable
    @SerialName("markdownBlock")
    data class MarkdownBlock(val node: MarkdownNodeDto) : SegmentDto()

    /** Fallback for any discriminator the renderer doesn't understand. */
    @Serializable
    @SerialName("unknown")
    data class Unknown(val type: String = "unknown") : SegmentDto()
}

internal object SegmentDtoSerializer :
    JsonContentPolymorphicSerializer<SegmentDto>(SegmentDto::class) {
    override fun selectDeserializer(element: JsonElement) =
        when (element.jsonObject["type"]?.jsonPrimitive?.content) {
            "text" -> SegmentDto.Text.serializer()
            "mention" -> SegmentDto.Mention.serializer()
            "eventRef" -> SegmentDto.EventRef.serializer()
            "hashtag" -> SegmentDto.Hashtag.serializer()
            "url" -> SegmentDto.Url.serializer()
            "media" -> SegmentDto.Media.serializer()
            "emoji" -> SegmentDto.Emoji.serializer()
            "invoice" -> SegmentDto.Invoice.serializer()
            "markdownBlock" -> SegmentDto.MarkdownBlock.serializer()
            else -> SegmentDto.Unknown.serializer()
        }
}
