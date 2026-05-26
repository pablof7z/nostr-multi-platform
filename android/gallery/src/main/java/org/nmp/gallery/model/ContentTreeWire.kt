package org.nmp.gallery.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator

@Serializable
data class ContentTreeWire(
    val nodes: List<WireNode> = emptyList(),
    val roots: List<UInt> = emptyList(),
    val mode: String? = null,
) {
    fun nodeAt(index: UInt): WireNode? {
        val value = index.toLong()
        if (value < 0 || value >= nodes.size.toLong()) return null
        return nodes[value.toInt()]
    }

    fun withRoots(nextRoots: List<UInt>): ContentTreeWire =
        copy(roots = nextRoots)
}

@Serializable
enum class MediaKind {
    @SerialName("Image") Image,
    @SerialName("Video") Video,
    @SerialName("Audio") Audio,
}

@Serializable
enum class WireNostrUriKind {
    @SerialName("profile") Profile,
    @SerialName("event") Event,
    @SerialName("address") Address,
}

@Serializable
enum class PlaceholderReason {
    @SerialName("depth_limit") DepthLimit,
    @SerialName("unresolved_uri") UnresolvedUri,
}

@Serializable
data class WireInvoice(
    @SerialName("Bolt11") val bolt11: String? = null,
    @SerialName("Bolt12") val bolt12: String? = null,
    @SerialName("Cashu") val cashu: String? = null,
)

@Serializable
data class WireNostrUri(
    val uri: String,
    val kind: WireNostrUriKind,
    @SerialName("primary_id") val primaryId: String,
    val relays: List<String> = emptyList(),
    val author: String? = null,
    @SerialName("event_kind") val eventKind: UInt? = null,
)

@OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
@Serializable
@JsonClassDiscriminator("kind")
sealed class WireNode {
    @Serializable
    @SerialName("text")
    data class Text(val text: String) : WireNode()

    @Serializable
    @SerialName("mention")
    data class Mention(val uri: WireNostrUri) : WireNode()

    @Serializable
    @SerialName("event_ref")
    data class EventRef(val uri: WireNostrUri) : WireNode()

    @Serializable
    @SerialName("hashtag")
    data class Hashtag(val tag: String) : WireNode()

    @Serializable
    @SerialName("url")
    data class Url(val url: String) : WireNode()

    @Serializable
    @SerialName("media")
    data class Media(
        val urls: List<String>,
        @SerialName("media_kind") val mediaKind: MediaKind,
    ) : WireNode()

    @Serializable
    @SerialName("emoji")
    data class Emoji(
        val shortcode: String,
        val url: String? = null,
    ) : WireNode()

    @Serializable
    @SerialName("invoice")
    data class Invoice(val invoice: WireInvoice) : WireNode()

    @Serializable
    @SerialName("heading")
    data class Heading(
        val level: UByte,
        val children: List<UInt>,
    ) : WireNode()

    @Serializable
    @SerialName("paragraph")
    data class Paragraph(val children: List<UInt>) : WireNode()

    @Serializable
    @SerialName("block_quote")
    data class BlockQuote(val children: List<UInt>) : WireNode()

    @Serializable
    @SerialName("code_block")
    data class CodeBlock(
        val info: String? = null,
        val body: String,
    ) : WireNode()

    @Serializable
    @SerialName("list")
    data class ListNode(
        @SerialName("ordered_start") val orderedStart: ULong? = null,
        val items: List<List<UInt>>,
    ) : WireNode()

    @Serializable
    @SerialName("rule")
    object Rule : WireNode()

    @Serializable
    @SerialName("emphasis")
    data class Emphasis(val children: List<UInt>) : WireNode()

    @Serializable
    @SerialName("strong")
    data class Strong(val children: List<UInt>) : WireNode()

    @Serializable
    @SerialName("inline_code")
    data class InlineCode(val code: String) : WireNode()

    @Serializable
    @SerialName("link")
    data class Link(
        val children: List<UInt>,
        val href: String? = null,
    ) : WireNode()

    @Serializable
    @SerialName("image")
    data class Image(
        val alt: String,
        val title: String? = null,
        val src: String? = null,
    ) : WireNode()

    @Serializable
    @SerialName("soft_break")
    object SoftBreak : WireNode()

    @Serializable
    @SerialName("hard_break")
    object HardBreak : WireNode()

    @Serializable
    @SerialName("placeholder")
    data class Placeholder(val reason: PlaceholderReason) : WireNode()
}
