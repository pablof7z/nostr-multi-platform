package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Kotlin port of the Swift `MarkdownNodeDto` / `MarkdownInlineDto` enums.
 * Mirrors `crates/nmp-nip23::Markdown*` via the STAGE 2 serde projection.
 * CommonMark core only (PD-012): tables / strikethrough arrive as literal
 * text per the verified `Options::empty()` behaviour.
 */
@Serializable(with = MarkdownNodeDtoSerializer::class)
sealed class MarkdownNodeDto {
    @Serializable
    @SerialName("heading")
    data class Heading(
        val level: Int,
        val inlines: List<MarkdownInlineDto>,
    ) : MarkdownNodeDto()

    @Serializable
    @SerialName("paragraph")
    data class Paragraph(val inlines: List<MarkdownInlineDto>) : MarkdownNodeDto()

    @Serializable
    @SerialName("blockQuote")
    data class BlockQuote(val blocks: List<MarkdownNodeDto>) : MarkdownNodeDto()

    @Serializable
    @SerialName("codeBlock")
    data class CodeBlock(
        val info: String? = null,
        val body: String,
    ) : MarkdownNodeDto()

    @Serializable
    @SerialName("list")
    data class ListNode(
        @SerialName("ordered_start") val orderedStart: Long? = null,
        val items: List<List<MarkdownNodeDto>>,
    ) : MarkdownNodeDto()

    @Serializable
    @SerialName("rule")
    object Rule : MarkdownNodeDto()

    @Serializable
    @SerialName("unknown")
    data class Unknown(val type: String = "unknown") : MarkdownNodeDto()
}

internal object MarkdownNodeDtoSerializer :
    JsonContentPolymorphicSerializer<MarkdownNodeDto>(MarkdownNodeDto::class) {
    override fun selectDeserializer(element: JsonElement) =
        when (element.jsonObject["type"]?.jsonPrimitive?.content) {
            "heading" -> MarkdownNodeDto.Heading.serializer()
            "paragraph" -> MarkdownNodeDto.Paragraph.serializer()
            "blockQuote" -> MarkdownNodeDto.BlockQuote.serializer()
            "codeBlock" -> MarkdownNodeDto.CodeBlock.serializer()
            "list" -> MarkdownNodeDto.ListNode.serializer()
            "rule" -> MarkdownNodeDto.Rule.serializer()
            else -> MarkdownNodeDto.Unknown.serializer()
        }
}

@Serializable(with = MarkdownInlineDtoSerializer::class)
sealed class MarkdownInlineDto {
    @Serializable
    @SerialName("inline")
    data class Inline(val segment: SegmentDto) : MarkdownInlineDto()

    @Serializable
    @SerialName("emphasis")
    data class Emphasis(val children: List<MarkdownInlineDto>) : MarkdownInlineDto()

    @Serializable
    @SerialName("strong")
    data class Strong(val children: List<MarkdownInlineDto>) : MarkdownInlineDto()

    @Serializable
    @SerialName("code")
    data class Code(val text: String) : MarkdownInlineDto()

    @Serializable
    @SerialName("link")
    data class Link(
        val label: List<MarkdownInlineDto>,
        val href: String? = null,
    ) : MarkdownInlineDto()

    @Serializable
    @SerialName("image")
    data class Image(
        val alt: String,
        val title: String? = null,
        val src: String? = null,
    ) : MarkdownInlineDto()

    @Serializable
    @SerialName("softBreak")
    object SoftBreak : MarkdownInlineDto()

    @Serializable
    @SerialName("hardBreak")
    object HardBreak : MarkdownInlineDto()

    @Serializable
    @SerialName("unknown")
    data class Unknown(val type: String = "unknown") : MarkdownInlineDto()
}

internal object MarkdownInlineDtoSerializer :
    JsonContentPolymorphicSerializer<MarkdownInlineDto>(MarkdownInlineDto::class) {
    override fun selectDeserializer(element: JsonElement) =
        when (element.jsonObject["type"]?.jsonPrimitive?.content) {
            "inline" -> MarkdownInlineDto.Inline.serializer()
            "emphasis" -> MarkdownInlineDto.Emphasis.serializer()
            "strong" -> MarkdownInlineDto.Strong.serializer()
            "code" -> MarkdownInlineDto.Code.serializer()
            "link" -> MarkdownInlineDto.Link.serializer()
            "image" -> MarkdownInlineDto.Image.serializer()
            "softBreak" -> MarkdownInlineDto.SoftBreak.serializer()
            "hardBreak" -> MarkdownInlineDto.HardBreak.serializer()
            else -> MarkdownInlineDto.Unknown.serializer()
        }
}
