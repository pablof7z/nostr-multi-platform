package org.nmp.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp

/**
 * Compose port of the Swift `NostrRichText` live renderer
 * while keeping rendering policy in Rust-produced payloads.
 *
 * Renders plain text that may contain `nostr:` URI mentions and event
 * references. Used by surfaces that don't run full markdown — profile
 * bios, room descriptions, chat messages, the live timeline.
 *
 * Strategy:
 *   1. Tokenise the input into `.text(…)` / `.profileMention(…)` /
 *      `.eventRef(…)` runs by scanning for `nostr:` URI prefixes.
 *   2. Group runs into paragraphs split at event-ref runs — profile
 *      mentions stay inline (concatenated into the surrounding `Text`),
 *      event refs become block cards.
 *   3. Entity resolution is not yet available on Android (the FFI doesn't
 *      expose decode/resolve), so event-ref cards render as a bounded
 *      loading stub — the same shape iOS shows before resolution.
 *
 * The tokenizer is a byte-for-byte port of the iOS `tokenise` /
 * `scanNostrURI` pair so both platforms recognise the same URIs.
 */
private val HRP_PREFIXES = listOf(
    "npub1",
    "nprofile1",
    "note1",
    "nevent1",
    "naddr1",
)

private sealed class Run {
    data class PlainText(val text: String) : Run()
    /** `npub1…` / `nprofile1…` — rendered inline as an `@npub…` chip. */
    data class ProfileMention(val raw: String) : Run()
    /** `note1…` / `nevent1…` / `naddr1…` — block event-ref card. */
    data class EventRef(val raw: String) : Run()
}

private sealed class Block {
    data class Paragraph(val runs: List<Run>) : Block()
    data class EventRefBlock(val raw: String) : Block()
}

@Composable
fun NostrRichText(
    content: String,
    modifier: Modifier = Modifier,
) {
    val blocks = remember(content) { groupBlocks(tokenise(content)) }
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        for (block in blocks) {
            when (block) {
                is Block.Paragraph -> Paragraph(block.runs)
                is Block.EventRefBlock -> EventRefStub(block.raw)
            }
        }
    }
}

@Composable
private fun Paragraph(runs: List<Run>) {
    val text = buildAnnotatedString {
        for (run in runs) {
            when (run) {
                is Run.PlainText -> append(run.text)
                is Run.ProfileMention -> {
                    val label = "@npub1${run.raw.removeBech32HrpPrefix().take(6)}…"
                    withStyle(
                        SpanStyle(
                            color = MentionAccent,
                            fontWeight = FontWeight.Bold,
                        ),
                    ) { append(label) }
                }
                is Run.EventRef -> Unit // unreachable: split into its own block.
            }
        }
    }
    Text(text, style = MaterialTheme.typography.bodyMedium)
}

@Composable
private fun EventRefStub(raw: String) {
    val short = raw.take(20) + "…"
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .border(
                width = 1.dp,
                color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f),
                shape = RoundedCornerShape(8.dp),
            )
            .background(Color.Gray.copy(alpha = 0.06f))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            "Quoted event",
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            short,
            style = MaterialTheme.typography.labelSmall.copy(
                fontFamily = FontFamily.Monospace,
            ),
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
        )
        Text(
            "(resolution pending — FFI decode not yet exposed)",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
        )
    }
}

private val MentionAccent = Color(0xFF5856D6) // SwiftUI Color.indigo parity

// MARK: - Tokeniser (port of iOS scanNostrURI + tokenise)

/**
 * Walks `content`, emitting a sequence of [Run.PlainText] /
 * [Run.ProfileMention] / [Run.EventRef] runs by matching the canonical
 * NIP-21 URI shape: `nostr:` (case-insensitive) followed by an HRP prefix
 * (`npub1`, `nprofile1`, `note1`, `nevent1`, `naddr1`) and a bech32 body
 * (lowercase ASCII alphanumerics — `[a-z0-9]+`).
 *
 * Mirrors the iOS implementation exactly so the same input produces the
 * same token sequence on both platforms.
 */
private fun tokenise(content: String): List<Run> {
    val out = mutableListOf<Run>()
    var i = 0
    while (i < content.length) {
        val match = scanNostrUri(content, i)
        if (match == null) {
            // No further URI — flush the rest as plain text.
            if (i < content.length) out.add(Run.PlainText(content.substring(i)))
            break
        }
        // Preserve any plain text preceding the URI.
        if (match.start > i) out.add(Run.PlainText(content.substring(i, match.start)))
        out.add(classify(match.body))
        i = match.end
    }
    return out
}

private data class UriMatch(val start: Int, val end: Int, val body: String)

private fun scanNostrUri(s: String, from: Int): UriMatch? {
    val prefix = findCaseInsensitive(s, "nostr:", from) ?: return null
    val bodyStart = prefix + "nostr:".length
    var end = bodyStart
    while (end < s.length && isBech32Char(s[end])) end++
    if (end == bodyStart) {
        // "nostr:" with no body — keep scanning past it (iOS skips by
        // returning nil; the outer loop falls through to plain text).
        return null
    }
    val body = s.substring(bodyStart, end)
    val lower = body.lowercase()
    if (HRP_PREFIXES.none { lower.startsWith(it) }) return null
    return UriMatch(start = prefix, end = end, body = body)
}

private fun findCaseInsensitive(s: String, needle: String, from: Int): Int? {
    if (needle.isEmpty()) return from
    val n = needle.lowercase()
    var i = from
    val limit = s.length - n.length
    while (i <= limit) {
        var ok = true
        for (k in n.indices) {
            if (s[i + k].lowercaseChar() != n[k]) { ok = false; break }
        }
        if (ok) return i
        i++
    }
    return null
}

private fun isBech32Char(c: Char): Boolean {
    // Lowercase ASCII letters + digits (bech32 alphabet is a subset; the
    // bounded set is fine here — invalid bodies fall through downstream).
    val v = c.code
    return (v in 0x30..0x39) || (v in 0x61..0x7A)
}

private fun classify(body: String): Run {
    val lower = body.lowercase()
    return when {
        lower.startsWith("npub1") || lower.startsWith("nprofile1") ->
            Run.ProfileMention(body)
        lower.startsWith("note1") || lower.startsWith("nevent1")
            || lower.startsWith("naddr1") ->
            Run.EventRef(body)
        else -> Run.PlainText(body) // unreachable per scanNostrUri filter.
    }
}

private fun groupBlocks(runs: List<Run>): List<Block> {
    val out = mutableListOf<Block>()
    val pending = mutableListOf<Run>()
    for (run in runs) {
        when (run) {
            is Run.PlainText, is Run.ProfileMention -> pending.add(run)
            is Run.EventRef -> {
                if (pending.isNotEmpty()) {
                    out.add(Block.Paragraph(pending.toList()))
                    pending.clear()
                }
                out.add(Block.EventRefBlock(run.raw))
            }
        }
    }
    if (pending.isNotEmpty()) out.add(Block.Paragraph(pending.toList()))
    return out
}

private fun String.removeBech32HrpPrefix(): String {
    val lower = lowercase()
    val hrp = HRP_PREFIXES.firstOrNull { lower.startsWith(it) } ?: return this
    return substring(hrp.length)
}
