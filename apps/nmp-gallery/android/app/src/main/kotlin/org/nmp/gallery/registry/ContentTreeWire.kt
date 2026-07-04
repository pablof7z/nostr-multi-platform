// Requires: kotlinx-serialization-json (>= 1.6), Kotlin 1.9+
//
// Kotlin mirror of the Rust `nmp_content::wire::ContentTreeWire`. See
// `crates/nmp-content/src/wire/mod.rs` for the canonical definition.
//
// The Rust type is a flat *arena*: `nodes: Vec<WireNode>` plus
// `roots: Vec<u32>`. Every recursive parent → child edge in the source tree is
// a `Vec<u32>` of indices into `nodes`. This file is shipped as a registry
// component so apps that install `compose/content-core` get a complete,
// drift-resistant mirror without hand-rolling Decodable equivalents.
//
// Discriminator strategy: `WireNode` uses `@JsonClassDiscriminator("kind")` to
// match the Rust `#[serde(tag = "kind", rename_all = "snake_case")]`. Apps
// configuring their own `Json` instance must keep `classDiscriminator = "kind"`
// (or pass `useArrayPolymorphism = false`, the default).

package org.nmp.gallery.registry

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp

@Serializable
public data class ContentTreeWire(
    /** Flat arena of every node in the tree (block + inline kinds). */
    val nodes: List<WireNode> = emptyList(),
    /** Top-level node indices, in document order. */
    val roots: List<UInt> = emptyList(),
    /** The mode the source tree was produced under (mirrors `RenderMode`). */
    val mode: String? = null,
) {
    /** Bounds-checked arena lookup. Returns `null` for out-of-range indices. */
    public fun nodeAt(index: UInt): WireNode? {
        val i = index.toLong()
        if (i < 0 || i >= nodes.size.toLong()) return null
        return nodes[i.toInt()]
    }
}

/**
 * Typed mirror of the Rust `nmp_content::MediaKind` enum. Serialized values
 * match the PascalCase Rust serde representation (the Rust enum has no
 * `rename_all`).
 */
@Serializable
public enum class MediaKind {
    @SerialName("Image") Image,
    @SerialName("Video") Video,
    @SerialName("Audio") Audio,
}

/** NIP-21 entity discriminator on the wire. */
@Serializable
public enum class WireNostrUriKind {
    @SerialName("profile") Profile,
    @SerialName("event") Event,
    @SerialName("address") Address,
}

/** Why a `placeholder` wire node was emitted. */
@Serializable
public enum class PlaceholderReason {
    @SerialName("depth_limit") DepthLimit,
    @SerialName("unresolved_uri") UnresolvedUri,
}

/**
 * Reserved payment segment (`WireNode::Invoice`).
 *
 * The Rust `InvoiceKind` enum has bare `Serialize`/`Deserialize` derives
 * (no `#[serde(tag = "…")]`), so serde emits the default
 * **externally-tagged** form — one of:
 *
 *     {"Bolt11": "lnbc…"}
 *     {"Bolt12": "lno…"}
 *     {"Cashu":  "cashuA…"}
 *
 * We mirror that on the wire as three mutually-exclusive optional fields
 * rather than a `sealed class`, because `kotlinx.serialization`'s default
 * polymorphic encoding for sealed classes is *internally* tagged (`"type"`
 * key + payload fields at the same level), which would not decode the
 * externally-tagged JSON Rust actually emits. Exactly one of the three
 * fields will be present in a well-formed payload.
 */
@Serializable
public data class WireInvoice(
    @SerialName("Bolt11") val bolt11: String? = null,
    @SerialName("Bolt12") val bolt12: String? = null,
    @SerialName("Cashu") val cashu: String? = null,
)

/**
 * Flattened, Serializable projection of `nmp_core::nip21::NostrUri`. `uri` is
 * the round-trippable canonical `nostr:` URI; `primaryId` is the hex pubkey
 * for profiles, event id for events, or author pubkey for addresses.
 */
@Serializable
public data class WireNostrUri(
    val uri: String,
    val kind: WireNostrUriKind,
    @SerialName("primary_id") val primaryId: String,
    val relays: List<String> = emptyList(),
    val author: String? = null,
    @SerialName("event_kind") val eventKind: UInt? = null,
)

/**
 * One node in the [ContentTreeWire] arena. Covers every variant of the Rust
 * `WireNode` enum. The class discriminator field is `kind` to match the Rust
 * `#[serde(tag = "kind")]` representation.
 */
@OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
@Serializable
@JsonClassDiscriminator("kind")
public sealed class WireNode {
    /** Inline text run. */
    @Serializable
    @SerialName("text")
    public data class Text(val text: String) : WireNode()

    /** Profile mention. */
    @Serializable
    @SerialName("mention")
    public data class Mention(val uri: WireNostrUri) : WireNode()

    /** Event / address reference. */
    @Serializable
    @SerialName("event_ref")
    public data class EventRef(val uri: WireNostrUri) : WireNode()

    /** `#hashtag` without leading `#`. */
    @Serializable
    @SerialName("hashtag")
    public data class Hashtag(val tag: String) : WireNode()

    /** Plain URL. */
    @Serializable
    @SerialName("url")
    public data class Url(val url: String) : WireNode()

    /** Ad-candidate URL. Renders exactly like [Url] — a plain link. */
    @Serializable
    @SerialName("ad_candidate_url")
    public data class AdCandidateUrl(val url: String) : WireNode()

    /** Grouped media block. */
    @Serializable
    @SerialName("media")
    public data class Media(
        val urls: List<String>,
        @SerialName("media_kind") val mediaKind: MediaKind,
    ) : WireNode()

    /** NIP-30 custom emoji. */
    @Serializable
    @SerialName("emoji")
    public data class Emoji(
        val shortcode: String,
        val url: String? = null,
    ) : WireNode()

    /** Reserved payment segment. */
    @Serializable
    @SerialName("invoice")
    public data class Invoice(val invoice: WireInvoice) : WireNode()

    /** Markdown heading. */
    @Serializable
    @SerialName("heading")
    public data class Heading(
        val level: UByte,
        val children: List<UInt>,
    ) : WireNode()

    /** Markdown paragraph. */
    @Serializable
    @SerialName("paragraph")
    public data class Paragraph(val children: List<UInt>) : WireNode()

    /** Markdown block quote. */
    @Serializable
    @SerialName("block_quote")
    public data class BlockQuote(val children: List<UInt>) : WireNode()

    /** Markdown fenced/indented code block (verbatim, never tokenized). */
    @Serializable
    @SerialName("code_block")
    public data class CodeBlock(
        val info: String? = null,
        val body: String,
    ) : WireNode()

    /** Markdown bullet/ordered list. */
    @Serializable
    @SerialName("list")
    public data class ListNode(
        @SerialName("ordered_start") val orderedStart: ULong? = null,
        val items: List<List<UInt>>,
    ) : WireNode()

    /** Markdown horizontal rule. */
    @Serializable
    @SerialName("rule")
    public object Rule : WireNode()

    /** `*italic*` — children are inline node indices. */
    @Serializable
    @SerialName("emphasis")
    public data class Emphasis(val children: List<UInt>) : WireNode()

    /** `**bold**` — children are inline node indices. */
    @Serializable
    @SerialName("strong")
    public data class Strong(val children: List<UInt>) : WireNode()

    /** Inline `` `code` `` (verbatim). */
    @Serializable
    @SerialName("inline_code")
    public data class InlineCode(val code: String) : WireNode()

    /** `[label](href)` — `label` children are inline node indices. */
    @Serializable
    @SerialName("link")
    public data class Link(
        val children: List<UInt>,
        val href: String? = null,
    ) : WireNode()

    /** `![alt](src "title")`. */
    @Serializable
    @SerialName("image")
    public data class Image(
        val alt: String,
        val title: String? = null,
        val src: String? = null,
    ) : WireNode()

    /** Soft line break. */
    @Serializable
    @SerialName("soft_break")
    public object SoftBreak : WireNode()

    /** Hard line break. */
    @Serializable
    @SerialName("hard_break")
    public object HardBreak : WireNode()

    /**
     * Typed placeholder: content existed here but could not be projected.
     * Never a dropped subtree — always a renderable node.
     */
    @Serializable
    @SerialName("placeholder")
    public data class Placeholder(val reason: PlaceholderReason) : WireNode()
}

/**
 * Deterministic identicon helpers for a hex pubkey.
 *
 * Algorithm is the 5×5 symmetric djb2 grid (GitHub-style): the lower 15 bits
 * of the djb2 hash determine which of the 15 left-half cells (3 cols × 5 rows)
 * are filled; columns 3–4 mirror columns 1–0 so the result is horizontally
 * symmetric. Color uses djb2 % 360 as the HSV hue with S=0.55, V=0.75.
 *
 * This algorithm is byte-identical to the Swift implementation in
 * `swiftui/content-core/ContentTreeWire.swift` and
 * `swiftui/user-avatar/NostrAvatar.swift`. Same pubkey → same grid and color
 * on every platform.
 */
public object NostrIdenticon {
    /** Stable HSV [Color] derived from a hex pubkey (or any string).
     *  Uses djb2 hash mapped to hue with fixed S=0.55, V=0.75 for legibility.
     */
    public fun colorForPubkey(pubkey: String): Color {
        val hue = (djb2(pubkey) % 360u).toFloat()
        return Color.hsv(hue, 0.55f, 0.75f)
    }

    /**
     * Returns a 5×5 symmetric fill pattern for a pubkey. Row-major: `cells[row][col]`
     * is `true` iff that cell is filled. The left 3 columns (0–2) are determined by
     * the lower 15 bits of djb2; columns 3–4 mirror columns 1–0.
     */
    public fun cellsForPubkey(pubkey: String): Array<BooleanArray> {
        val hash = djb2(pubkey)
        return Array(5) { row ->
            BooleanArray(5) { col ->
                val mirrorCol = if (col < 3) col else 4 - col
                val bit = row * 3 + mirrorCol
                (hash shr bit) and 1u == 1u
            }
        }
    }

    /**
     * Two-character monogram derived from a hex pubkey (first two chars,
     * uppercased). Apps with kind:0 profile data should provide their own.
     */
    public fun initialsForPubkey(pubkey: String): String {
        val trimmed = pubkey.trim()
        if (trimmed.isEmpty()) return "?"
        return trimmed.take(2).uppercase()
    }

    private fun djb2(value: String): UInt {
        var hash: UInt = 5381u
        for (byte in value.encodeToByteArray()) {
            hash = hash * 33u + byte.toUByte().toUInt()
        }
        return hash
    }
}

/**
 * Renders the 5×5 symmetric identicon grid for [pubkey] using [NostrIdenticon].
 * Filled cells are drawn in the pubkey color; empty cells show the same color
 * at 15% opacity so the tile reads as a tinted patch rather than blank.
 */
@Composable
public fun NostrIdenticonGrid(pubkey: String, size: Dp, modifier: Modifier = Modifier) {
    val color = NostrIdenticon.colorForPubkey(pubkey)
    val cells = remember(pubkey) { NostrIdenticon.cellsForPubkey(pubkey) }
    Canvas(
        modifier = modifier
            .size(size)
            .background(color.copy(alpha = 0.15f)),
    ) {
        val spacing = 1f
        val cellPx = (minOf(this.size.width, this.size.height) - spacing * 4f) / 5f
        for (row in 0..4) {
            for (col in 0..4) {
                if (cells[row][col]) {
                    drawRect(
                        color = color,
                        topLeft = Offset(col * (cellPx + spacing), row * (cellPx + spacing)),
                        size = Size(cellPx, cellPx),
                    )
                }
            }
        }
    }
}
