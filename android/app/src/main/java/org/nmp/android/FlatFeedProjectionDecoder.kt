package org.nmp.android

import nmp.transport.Pair
import nmp.transport.Value
import nmp.transport.ValueKind
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.ChirpReplyAttribution
import org.nmp.android.model.ChirpRootCard
import org.nmp.android.model.TimelineWindowCursor
import org.nmp.android.model.TimelineWindowPage

internal object FlatFeedProjectionDecoder {
    fun decode(projections: Map<String, Value>): Map<String, ChirpOpFeedSnapshot> {
        val result = HashMap<String, ChirpOpFeedSnapshot>()
        for ((key, value) in projections) {
            if (!key.startsWith("nmp.feed.author.") && !key.startsWith("nmp.feed.thread.")) {
                continue
            }
            decodeSnapshot(value)?.let { result[key] = it }
        }
        return result
    }

    private fun decodeSnapshot(v: Value): ChirpOpFeedSnapshot? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return ChirpOpFeedSnapshot(
            cards = m["cards"]?.listOf { decodeRootCard(it) } ?: emptyList(),
            page = m["page"]?.let { decodePage(it) },
        )
    }

    private fun decodeRootCard(v: Value): ChirpRootCard? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        val card = m["card"]?.let { decodeCard(it) } ?: return null
        return ChirpRootCard(
            card = card,
            attribution = m["attribution"]?.listOf { decodeAttribution(it) } ?: emptyList(),
        )
    }

    private fun decodeCard(v: Value): ChirpEventCard? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return ChirpEventCard(
            id = m["id"]?.stringOr("") ?: "",
            authorPubkey = m["authorPubkey"]?.stringOr("") ?: "",
            kind = m["kind"]?.intOr(0) ?: 0,
            createdAt = m["createdAt"]?.longOr(0L) ?: 0L,
            content = m["content"]?.stringOr("") ?: "",
            contentTree = null,
            authorDisplayName = m["authorDisplayName"]?.stringOrNull(),
            authorPictureUrl = m["authorPictureUrl"]?.stringOrNull(),
            contentPreview = m["contentPreview"]?.stringOr("") ?: "",
        )
    }

    private fun decodeAttribution(v: Value): ChirpReplyAttribution? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return ChirpReplyAttribution(
            authorPubkey = m["authorPubkey"]?.stringOr("") ?: "",
            authorDisplayName = m["authorDisplayName"]?.stringOrNull(),
            authorPictureUrl = m["authorPictureUrl"]?.stringOrNull(),
            replyEventId = m["replyEventId"]?.stringOr("") ?: "",
            replyCreatedAt = m["replyCreatedAt"]?.uLongOr(0UL) ?: 0UL,
        )
    }

    private fun decodePage(v: Value): TimelineWindowPage? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return TimelineWindowPage(
            limit = m["limit"]?.uLongOr(0UL) ?: 0UL,
            nextCursor = m["nextCursor"]?.let { decodeCursor(it) },
            hasMore = m["hasMore"]?.boolOr(false) ?: false,
            totalBlocks = m["totalBlocks"]?.uLongOr(0UL) ?: 0UL,
        )
    }

    private fun decodeCursor(v: Value): TimelineWindowCursor? {
        if (v.kind != ValueKind.Map) return null
        val m = buildValueMap(v)
        return TimelineWindowCursor(
            createdAt = m["createdAt"]?.uLongOr(0UL) ?: 0UL,
            id = m["id"]?.stringOr("") ?: "",
        )
    }

    private fun buildValueMap(v: Value): Map<String, Value> {
        val len = v.mapLength
        if (len == 0) return emptyMap()
        val result = HashMap<String, Value>(len * 2)
        for (i in 0 until len) {
            val pair: Pair = v.map(i) ?: continue
            val value: Value = pair.value ?: continue
            result[convertFromSnakeCase(pair.key)] = value
        }
        return result
    }

    private fun convertFromSnakeCase(key: String): String {
        if (!key.contains('_')) return key
        val leadingCount = key.indexOfFirst { it != '_' }.takeIf { it >= 0 } ?: return key
        val trailingCount = key.reversed().indexOfFirst { it != '_' }.takeIf { it >= 0 } ?: 0
        val start = leadingCount
        val end = key.length - trailingCount
        if (start >= end) return key
        val body = key.substring(start, end)
        val sb = StringBuilder(body.length)
        var capitalizeNext = false
        for (ch in body) {
            when {
                ch == '_' -> if (sb.isNotEmpty()) capitalizeNext = true
                capitalizeNext -> {
                    sb.append(ch.uppercaseChar())
                    capitalizeNext = false
                }
                else -> sb.append(ch)
            }
        }
        return key.substring(0, start) + sb.toString() + key.substring(end)
    }

    private fun Value.longOr(default: Long): Long = when (kind) {
        ValueKind.Int -> intValue
        ValueKind.UInt -> uintValue.toLong()
        else -> default
    }

    private fun Value.intOr(default: Int): Int = longOr(default.toLong()).toInt()
    private fun Value.uLongOr(default: ULong): ULong = longOr(default.toLong()).toULong()

    private fun Value.boolOr(default: Boolean): Boolean = when (kind) {
        ValueKind.Bool -> boolValue
        else -> default
    }

    private fun Value.stringOr(default: String): String = when (kind) {
        ValueKind.String -> stringValue ?: default
        else -> default
    }

    private fun Value.stringOrNull(): String? = when (kind) {
        ValueKind.String -> stringValue
        ValueKind.Null -> null
        else -> null
    }

    private fun <T : Any> Value.listOf(decode: (Value) -> T?): List<T> {
        if (kind != ValueKind.List) return emptyList()
        val result = ArrayList<T>(listLength)
        for (i in 0 until listLength) {
            val item: Value = list(i) ?: continue
            val decoded = decode(item) ?: continue
            result.add(decoded)
        }
        return result
    }
}
