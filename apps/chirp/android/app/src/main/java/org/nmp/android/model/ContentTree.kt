package org.nmp.android.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonEncoder
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

@Serializable
data class ContentTreeWire(
    val nodes: List<ContentWireNode> = emptyList(),
    val roots: List<Int> = emptyList(),
    val mode: String = "",
)

@Serializable
data class WireNostrUri(
    val uri: String = "",
    val kind: String = "",
    val primaryId: String = "",
    val relays: List<String> = emptyList(),
    val author: String? = null,
    val eventKind: Int? = null,
)

@Serializable(with = ContentWireNodeSerializer::class)
sealed interface ContentWireNode {
    data class TextNode(val text: String) : ContentWireNode
    data class MentionNode(val uri: WireNostrUri) : ContentWireNode
    data class EventRefNode(val uri: WireNostrUri) : ContentWireNode
    data class HashtagNode(val tag: String) : ContentWireNode
    data class UrlNode(val url: String) : ContentWireNode
    data class MediaNode(val urls: List<String>, val mediaKind: String) : ContentWireNode
    data class EmojiNode(val shortcode: String, val url: String?) : ContentWireNode
    data class InvoiceNode(val invoiceKind: String, val payload: String) : ContentWireNode
    data class ParagraphNode(val children: List<Int>) : ContentWireNode
    data class HeadingNode(val level: Int, val children: List<Int>) : ContentWireNode
    data class EmphasisNode(val children: List<Int>) : ContentWireNode
    data class StrongNode(val children: List<Int>) : ContentWireNode
    data class InlineCodeNode(val code: String) : ContentWireNode
    data class LinkNode(val children: List<Int>, val href: String?) : ContentWireNode
    data class ImageNode(val alt: String, val title: String?, val src: String?) : ContentWireNode
    data class CodeBlockNode(val info: String?, val body: String) : ContentWireNode
    data class ListNode(val orderedStart: Long?, val items: List<List<Int>>) : ContentWireNode
    data class BlockQuoteNode(val children: List<Int>) : ContentWireNode
    data object RuleNode : ContentWireNode
    data object SoftBreakNode : ContentWireNode
    data object HardBreakNode : ContentWireNode
    data class PlaceholderNode(val reason: String = "depth_limit") : ContentWireNode
}

object ContentWireNodeSerializer : KSerializer<ContentWireNode> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("ContentWireNode")

    override fun deserialize(decoder: Decoder): ContentWireNode {
        val input = decoder as? JsonDecoder
            ?: throw SerializationException("ContentWireNode requires JSON")
        val obj = input.decodeJsonElement().jsonObject
        return when (obj["kind"]?.jsonPrimitive?.contentOrNull) {
            "text" -> ContentWireNode.TextNode(obj.string("text"))
            "mention" -> ContentWireNode.MentionNode(obj.uri(input, "uri"))
            "event_ref" -> ContentWireNode.EventRefNode(obj.uri(input, "uri"))
            "hashtag" -> ContentWireNode.HashtagNode(obj.string("tag"))
            "url" -> ContentWireNode.UrlNode(obj.string("url"))
            "media" -> ContentWireNode.MediaNode(obj.stringList("urls"), obj.string("media_kind"))
            "emoji" -> ContentWireNode.EmojiNode(obj.string("shortcode"), obj.optString("url"))
            "invoice" -> obj.invoiceNode()
            "paragraph" -> ContentWireNode.ParagraphNode(obj.indexList("children"))
            "heading" -> ContentWireNode.HeadingNode(obj.int("level"), obj.indexList("children"))
            "emphasis" -> ContentWireNode.EmphasisNode(obj.indexList("children"))
            "strong" -> ContentWireNode.StrongNode(obj.indexList("children"))
            "inline_code" -> ContentWireNode.InlineCodeNode(obj.string("code"))
            "link" -> ContentWireNode.LinkNode(obj.indexList("children"), obj.optString("href"))
            "image" -> ContentWireNode.ImageNode(obj.string("alt"), obj.optString("title"), obj.optString("src"))
            "code_block" -> ContentWireNode.CodeBlockNode(obj.optString("info"), obj.string("body"))
            "list" -> ContentWireNode.ListNode(obj.longOrNull("ordered_start"), obj.indexLists("items"))
            "block_quote" -> ContentWireNode.BlockQuoteNode(obj.indexList("children"))
            "rule" -> ContentWireNode.RuleNode
            "soft_break" -> ContentWireNode.SoftBreakNode
            "hard_break" -> ContentWireNode.HardBreakNode
            "placeholder" -> ContentWireNode.PlaceholderNode(obj.string("reason").ifEmpty { "depth_limit" })
            else -> ContentWireNode.PlaceholderNode()
        }
    }

    override fun serialize(encoder: Encoder, value: ContentWireNode) {
        val output = encoder as? JsonEncoder
            ?: throw SerializationException("ContentWireNode requires JSON")
        output.encodeJsonElement(buildJsonObject { put("kind", "placeholder") })
    }
}

private fun JsonObject.string(key: String): String =
    this[key]?.jsonPrimitive?.contentOrNull.orEmpty()

private fun JsonObject.optString(key: String): String? =
    this[key]?.jsonPrimitive?.contentOrNull

private fun JsonObject.int(key: String): Int =
    this[key]?.jsonPrimitive?.intOrNull ?: 0

private fun JsonObject.longOrNull(key: String): Long? =
    this[key]?.jsonPrimitive?.contentOrNull?.toLongOrNull()

private fun JsonObject.indexList(key: String): List<Int> =
    this[key]?.jsonArray?.mapNotNull { it.jsonPrimitive.intOrNull } ?: emptyList()

private fun JsonObject.indexLists(key: String): List<List<Int>> =
    this[key]?.jsonArray?.map { item ->
        item.jsonArray.mapNotNull { it.jsonPrimitive.intOrNull }
    } ?: emptyList()

private fun JsonObject.stringList(key: String): List<String> =
    this[key]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull } ?: emptyList()

private fun JsonObject.uri(input: JsonDecoder, key: String): WireNostrUri {
    val value = this[key] ?: return WireNostrUri()
    return input.json.decodeFromJsonElement(WireNostrUri.serializer(), value)
}

private fun JsonObject.invoiceNode(): ContentWireNode.InvoiceNode {
    val invoice = this["invoice"]?.jsonObject ?: return ContentWireNode.InvoiceNode("", "")
    val entry = invoice.entries.firstOrNull() ?: return ContentWireNode.InvoiceNode("", "")
    return ContentWireNode.InvoiceNode(entry.key, entry.value.jsonPrimitive.contentOrNull.orEmpty())
}
