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
import kotlinx.serialization.json.add
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

@Serializable
data class ChirpTimelineSnapshot(
    val blocks: List<TimelineBlock> = emptyList(),
    val cards: List<ChirpEventCard> = emptyList(),
)

@Serializable
data class ChirpEventCard(
    val id: String = "",
    val authorPubkey: String = "",
    val kind: Int = 0,
    val createdAt: Long = 0,
    val content: String = "",
    val contentTree: ContentTreeWire? = null,
    // aim.md §2 — display_name + picture_url are nullable: the
    // backend ships JSON null when no kind:0 has arrived for this
    // author, and the Compose layer is responsible for choosing its
    // own fallback (typically a short-pubkey abbreviation).
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val contentPreview: String = "",
)

@Serializable(with = TimelineBlockSerializer::class)
sealed interface TimelineBlock {
    val eventIds: List<String>
    val hasGap: Boolean
}

data class StandaloneTimelineBlock(val eventId: String) : TimelineBlock {
    override val eventIds: List<String> = listOf(eventId)
    override val hasGap: Boolean = false
}

data class ModuleTimelineBlock(
    val events: List<String>,
    override val hasGap: Boolean,
) : TimelineBlock {
    override val eventIds: List<String> = events
}

object TimelineBlockSerializer : KSerializer<TimelineBlock> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("TimelineBlock")

    override fun deserialize(decoder: Decoder): TimelineBlock {
        val input = decoder as? JsonDecoder
            ?: throw SerializationException("TimelineBlock requires JSON")
        val obj = input.decodeJsonElement().jsonObject
        obj["Standalone"]?.jsonPrimitive?.contentOrNull?.let {
            return StandaloneTimelineBlock(it)
        }
        obj["Module"]?.jsonObject?.let { module ->
            val events = module["events"]
                ?.jsonArray
                ?.mapNotNull { it.jsonPrimitive.contentOrNull }
                ?: emptyList()
            val hasGap = module["has_gap"]?.jsonPrimitive?.booleanOrNull ?: false
            return ModuleTimelineBlock(events = events, hasGap = hasGap)
        }
        throw SerializationException("unknown TimelineBlock variant")
    }

    override fun serialize(encoder: Encoder, value: TimelineBlock) {
        val output = encoder as? JsonEncoder
            ?: throw SerializationException("TimelineBlock requires JSON")
        val element = when (value) {
            is StandaloneTimelineBlock -> buildJsonObject {
                put("Standalone", value.eventId)
            }
            is ModuleTimelineBlock -> buildJsonObject {
                put(
                    "Module",
                    buildJsonObject {
                        put("events", buildJsonArray { value.events.forEach { add(it) } })
                        put("has_gap", value.hasGap)
                    },
                )
            }
        }
        output.encodeJsonElement(element)
    }
}
